use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};

use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, File, Metadata, OpenOptions},
};
use caseless::Caseless;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::{
    SkillFile, SkillManifest, manifest::parse_manifest, validate_file_path, zip_validation,
};

/// Maximum complete SKILL.md size, including frontmatter, before JSON serialization.
pub const MAX_INSTRUCTIONS_BYTES: usize = 256 * 1024;

/// Reserved root-relative record written by the Oceans skill installer.
pub const INSTALL_RECORD: &str = ".oceans-skill-lock.json";

/// Identify installer records using the same portable equivalence as bundle paths.
pub fn is_reserved_install_path(path: &str) -> bool {
    path.split('/')
        .next()
        .is_some_and(|root| portable_path_key(root) == INSTALL_RECORD)
}

fn portable_path_key(path: &str) -> String {
    path.nfd().default_case_fold().nfd().collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(default, deny_unknown_fields)]
pub struct BundleLimits {
    pub max_archive_bytes: u64,
    pub max_expanded_bytes: u64,
    pub max_files: u32,
}

impl Default for BundleLimits {
    fn default() -> Self {
        Self {
            max_archive_bytes: 10 * 1024 * 1024,
            max_expanded_bytes: 25 * 1024 * 1024,
            max_files: 1000,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BundleError {
    #[error("invalid skill manifest: {0}")]
    Manifest(String),
    #[error("invalid skill archive: {0}")]
    Archive(String),
    #[error("unsafe skill path: {0}")]
    UnsafePath(String),
    #[error("duplicate or conflicting skill path: {0}")]
    DuplicatePath(String),
    #[error("skill bundle exceeds {0} limit")]
    Limit(&'static str),
    #[error("skill bundle I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("skill ZIP failed: {0}")]
    Zip(#[from] zip::result::ZipError),
}

/// Validated bytes are never executed. File paths are relative to the skill root.
#[derive(Debug, Clone)]
pub struct ValidatedBundle {
    pub archive: Vec<u8>,
    pub sha256: String,
    pub manifest: SkillManifest,
    pub instructions: String,
    pub files: Vec<SkillFile>,
    pub contents: BTreeMap<String, Vec<u8>>,
    pub executable_files: BTreeSet<String>,
    pub extracted_bytes: u64,
}

/// Read a bounded ZIP and produce deterministic bytes in the Agent Skills layout.
pub fn inspect_archive(
    bytes: &[u8],
    limits: &BundleLimits,
) -> Result<ValidatedBundle, BundleError> {
    if bytes.len() as u64 > limits.max_archive_bytes {
        return Err(BundleError::Limit("compressed bytes"));
    }
    zip_validation::preflight(bytes, limits)?;
    let mut archive = ZipArchive::new(Cursor::new(bytes))?;
    let mut files = BundleFiles::default();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let name = std::str::from_utf8(entry.name_raw())
            .map_err(|_| BundleError::Archive("file paths must be UTF-8".into()))?
            .to_owned();
        let is_dir = entry.is_dir();
        let mode = entry.unix_mode().unwrap_or_default();
        if entry.encrypted()
            || entry.is_symlink()
            || !matches!(mode & 0o170000, 0 | 0o100000 | 0o040000)
            || (mode & 0o170000 == 0o040000 && !is_dir)
        {
            return Err(BundleError::UnsafePath(name));
        }
        let path = if is_dir {
            name.strip_suffix('/').unwrap_or(&name)
        } else {
            &name
        };
        files.register_path(path, is_dir)?;
        if is_dir {
            if entry.size() != 0 {
                return Err(BundleError::Archive(
                    "directory entries must be empty".into(),
                ));
            }
            continue;
        }
        let content = read_entry(&mut entry, path, &files, limits)?;
        files.insert(path.to_owned(), content, mode & 0o111 != 0, limits)?;
    }
    files.finish(limits, None)
}

/// Package a local skill directory without following symbolic or hard links.
pub fn pack_directory(path: &Path, limits: &BundleLimits) -> Result<ValidatedBundle, BundleError> {
    pack_directory_excluding(path, limits, &[])
}

/// Omit exact root-relative client metadata paths while retaining all validation.
pub fn pack_directory_excluding(
    path: &Path,
    limits: &BundleLimits,
    excluded_paths: &[&str],
) -> Result<ValidatedBundle, BundleError> {
    for excluded in excluded_paths {
        validate_file_path(excluded)?;
    }
    // Resolve only the parent through ambient authority. Opening the selected
    // directory itself and every descendant must reject replacement symlinks.
    let path = std::path::absolute(path)?;
    let wrapper = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| BundleError::UnsafePath(path.display().to_string()))?;
    let parent = Dir::open_ambient_dir(path.parent().unwrap(), ambient_authority())?;
    let root = parent.open_dir_nofollow(wrapper)?;
    let mut files = BundleFiles::default();
    let mut pending = vec![(PathBuf::new(), root)];
    let mut entry_count = 0u32;
    while let Some((relative_directory, directory)) = pending.pop() {
        for entry in directory.entries()? {
            let entry = entry?;
            let relative = relative_directory.join(entry.file_name());
            let name = relative
                .to_str()
                .ok_or_else(|| BundleError::UnsafePath(relative.display().to_string()))?
                .replace(std::path::MAIN_SEPARATOR, "/");
            if excluded_paths.contains(&name.as_str()) {
                continue;
            }
            entry_count = entry_count
                .checked_add(1)
                .ok_or(BundleError::Limit("file count"))?;
            if entry_count > limits.max_files {
                return Err(BundleError::Limit("archive entry count"));
            }
            let metadata = entry.metadata()?;
            if metadata.is_symlink() || (!metadata.is_dir() && !metadata.is_file()) {
                return Err(BundleError::UnsafePath(name));
            }
            files.register_path(&name, metadata.is_dir())?;
            if metadata.is_dir() {
                let child = directory.open_dir_nofollow(entry.file_name())?;
                pending.push((relative, child));
                continue;
            }
            let file = open_local_file(&directory, &entry.file_name(), &name)?;
            let executable = local_executable(&file.metadata()?);
            let content = read_entry(file, &name, &files, limits)?;
            files.insert(name, content, executable, limits)?;
        }
    }
    files.finish(limits, Some(wrapper))
}

#[derive(Default)]
struct BundleFiles {
    contents: BTreeMap<String, Vec<u8>>,
    executable_files: BTreeSet<String>,
    paths: BTreeMap<String, (String, bool)>,
    extracted_bytes: u64,
}

impl BundleFiles {
    fn register_path(&mut self, path: &str, is_dir: bool) -> Result<(), BundleError> {
        validate_file_path(path)?;
        let components: Vec<_> = path.split('/').collect();
        let mut prefix = String::new();
        for (index, component) in components.iter().enumerate() {
            if index != 0 {
                prefix.push('/');
            }
            prefix.push_str(component);
            let directory = index + 1 < components.len() || is_dir;
            let key = portable_path_key(&prefix);
            if let Some((existing, was_directory)) = self.paths.get(&key) {
                if existing != &prefix || !directory || !was_directory {
                    return Err(BundleError::DuplicatePath(path.into()));
                }
            } else {
                self.paths.insert(key, (prefix.clone(), directory));
            }
        }
        Ok(())
    }

    fn insert(
        &mut self,
        path: String,
        bytes: Vec<u8>,
        executable: bool,
        limits: &BundleLimits,
    ) -> Result<(), BundleError> {
        self.extracted_bytes = self
            .extracted_bytes
            .checked_add(bytes.len() as u64)
            .ok_or(BundleError::Limit("expanded bytes"))?;
        if self.extracted_bytes > limits.max_expanded_bytes {
            return Err(BundleError::Limit("expanded bytes"));
        }
        if self.contents.len() >= limits.max_files as usize {
            return Err(BundleError::Limit("file count"));
        }
        if executable {
            self.executable_files.insert(path.clone());
        }
        self.contents.insert(path, bytes);
        Ok(())
    }

    fn finish(
        self,
        limits: &BundleLimits,
        directory_name: Option<&str>,
    ) -> Result<ValidatedBundle, BundleError> {
        let skill_files: Vec<_> = self
            .contents
            .keys()
            .filter(|path| {
                path.rsplit('/')
                    .next()
                    .is_some_and(|name| name.eq_ignore_ascii_case("SKILL.md"))
            })
            .collect();
        if skill_files.len() != 1 {
            return Err(BundleError::Manifest(
                "bundle must contain exactly one SKILL.md".into(),
            ));
        }
        let skill_path = skill_files[0];
        let wrapper = match skill_path.split('/').collect::<Vec<_>>().as_slice() {
            ["SKILL.md"] => None,
            [name, "SKILL.md"] => Some((*name).to_owned()),
            _ => {
                return Err(BundleError::Manifest(
                    "SKILL.md must be at the root or in one enclosing folder".into(),
                ));
            }
        };
        let instruction_bytes = &self.contents[skill_path];
        if instruction_bytes.len() > MAX_INSTRUCTIONS_BYTES {
            return Err(BundleError::Limit("SKILL.md bytes (256 KiB)"));
        }
        if instruction_bytes.contains(&0) {
            return Err(BundleError::Manifest(
                "SKILL.md must not contain NUL bytes".into(),
            ));
        }
        let instructions = String::from_utf8(instruction_bytes.clone())
            .map_err(|_| BundleError::Manifest("SKILL.md must be UTF-8".into()))?;
        let manifest = parse_manifest(&instructions)?;
        if wrapper
            .as_deref()
            .or(directory_name)
            .is_some_and(|name| name != manifest.name)
        {
            return Err(BundleError::Manifest(
                "name must match the skill directory".into(),
            ));
        }
        let prefix = wrapper
            .as_ref()
            .map(|name| format!("{name}/"))
            .unwrap_or_default();
        if wrapper.as_ref().is_some_and(|name| {
            self.paths
                .values()
                .any(|(path, _)| path != name && !path.starts_with(&prefix))
        }) {
            return Err(BundleError::Manifest(
                "all paths must be inside the skill directory".into(),
            ));
        }
        if self.paths.values().any(|(path, _)| {
            path.strip_prefix(&prefix)
                .is_some_and(is_reserved_install_path)
        }) {
            return Err(BundleError::UnsafePath(format!(
                "reserved installer record {INSTALL_RECORD}"
            )));
        }
        let mut contents = BTreeMap::new();
        let mut executable_files = BTreeSet::new();
        for (path, bytes) in self.contents {
            let relative = path.strip_prefix(&prefix).ok_or_else(|| {
                BundleError::Manifest("all files must be inside the skill directory".into())
            })?;
            if self.executable_files.contains(&path) {
                executable_files.insert(relative.to_owned());
            }
            contents.insert(relative.to_owned(), bytes);
        }
        let archive = canonical_archive(&manifest.name, &contents, &executable_files)?;
        if archive.len() as u64 > limits.max_archive_bytes {
            return Err(BundleError::Limit("canonical compressed bytes"));
        }
        let sha256 = format!("{:x}", Sha256::digest(&archive));
        let files = contents
            .iter()
            .map(|(path, bytes)| SkillFile {
                path: path.clone(),
                size: bytes.len() as u64,
            })
            .collect();
        Ok(ValidatedBundle {
            archive,
            sha256,
            manifest,
            instructions,
            files,
            contents,
            executable_files,
            extracted_bytes: self.extracted_bytes,
        })
    }
}

fn canonical_archive(
    name: &str,
    contents: &BTreeMap<String, Vec<u8>>,
    executable: &BTreeSet<String>,
) -> Result<Vec<u8>, BundleError> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, bytes) in contents {
        let archive_path = format!("{name}/{path}");
        // The canonical wrapper also counts toward the same portable path limits.
        validate_file_path(&archive_path)?;
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .last_modified_time(zip::DateTime::default())
            .unix_permissions(if executable.contains(path) {
                0o755
            } else {
                0o644
            });
        writer.start_file(archive_path, options)?;
        writer.write_all(bytes)?;
    }
    Ok(writer.finish()?.into_inner())
}

fn read_entry(
    reader: impl Read,
    path: &str,
    files: &BundleFiles,
    limits: &BundleLimits,
) -> Result<Vec<u8>, BundleError> {
    let expanded_limit = limits
        .max_expanded_bytes
        .checked_sub(files.extracted_bytes)
        .ok_or(BundleError::Limit("expanded bytes"))?;
    let instructions = path
        .rsplit('/')
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case("SKILL.md"));
    let limit = if instructions {
        expanded_limit.min(MAX_INSTRUCTIONS_BYTES as u64)
    } else {
        expanded_limit
    };
    let mut bytes = Vec::new();
    reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(BundleError::Limit(
            if instructions && limit == MAX_INSTRUCTIONS_BYTES as u64 {
                "SKILL.md bytes (256 KiB)"
            } else {
                "expanded bytes"
            },
        ));
    }
    Ok(bytes)
}

fn open_local_file(
    directory: &Dir,
    filename: &std::ffi::OsStr,
    path: &str,
) -> Result<File, BundleError> {
    let mut options = OpenOptions::new();
    // Nonblocking open also prevents a replacement FIFO from hanging packaging.
    options.read(true).follow(FollowSymlinks::No).nonblock(true);
    let file = directory.open_with(filename, &options)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err(BundleError::UnsafePath(path.into()));
    }
    Ok(file)
}

#[cfg(unix)]
fn local_executable(metadata: &Metadata) -> bool {
    use cap_std::fs::MetadataExt;
    metadata.mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn local_executable(_metadata: &Metadata) -> bool {
    false
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::symlink};

    #[test]
    fn opened_directory_rejects_replacement_links_and_stays_anchored_after_rename() {
        let workspace = std::env::temp_dir().join(format!("skill-race-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(workspace.join("review/assets")).unwrap();
        fs::create_dir(workspace.join("outside")).unwrap();
        fs::write(workspace.join("review/file"), b"original").unwrap();
        fs::write(workspace.join("review/assets/notes"), b"inside").unwrap();
        fs::write(workspace.join("outside/file"), b"private").unwrap();
        fs::write(workspace.join("outside/notes"), b"private").unwrap();
        let directory =
            Dir::open_ambient_dir(workspace.join("review"), ambient_authority()).unwrap();

        // Simulate replacement after the walker inspected the entry metadata.
        assert!(directory.symlink_metadata("file").unwrap().is_file());
        fs::remove_file(workspace.join("review/file")).unwrap();
        symlink(
            workspace.join("outside/file"),
            workspace.join("review/file"),
        )
        .unwrap();
        assert!(open_local_file(&directory, "file".as_ref(), "file").is_err());
        fs::remove_file(workspace.join("review/file")).unwrap();
        fs::hard_link(
            workspace.join("outside/file"),
            workspace.join("review/file"),
        )
        .unwrap();
        assert!(open_local_file(&directory, "file".as_ref(), "file").is_err());

        let assets = directory.open_dir_nofollow("assets").unwrap();
        fs::rename(
            workspace.join("review/assets"),
            workspace.join("original-assets"),
        )
        .unwrap();
        symlink(workspace.join("outside"), workspace.join("review/assets")).unwrap();
        assert!(directory.open_dir_nofollow("assets").is_err());
        let mut contents = String::new();
        open_local_file(&assets, "notes".as_ref(), "assets/notes")
            .unwrap()
            .read_to_string(&mut contents)
            .unwrap();
        assert_eq!(contents, "inside");

        // Root replacement also cannot redirect a pinned directory handle.
        fs::rename(workspace.join("review"), workspace.join("original-review")).unwrap();
        symlink(workspace.join("outside"), workspace.join("review")).unwrap();
        assert!(pack_directory(&workspace.join("review"), &BundleLimits::default()).is_err());
        assert!(open_local_file(&directory, "file".as_ref(), "file").is_err());
        fs::remove_dir_all(workspace).unwrap();
    }
}
