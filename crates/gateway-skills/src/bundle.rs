use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};

use caseless::Caseless;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::{
    SkillFile, SkillManifest, manifest::parse_manifest, validate_file_path, zip_validation,
};

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
        let content = read_bounded(&mut entry, entry_size_limit(&files, limits)?)?;
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
    if !fs::symlink_metadata(path)?.file_type().is_dir() {
        return Err(BundleError::UnsafePath(path.display().to_string()));
    }
    let path = path.canonicalize()?;
    let wrapper = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| BundleError::UnsafePath(path.display().to_string()))?;
    let mut files = BundleFiles::default();
    let mut pending = vec![PathBuf::new()];
    let mut entry_count = 0u32;
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(path.join(&directory))? {
            let entry = entry?;
            entry_count = entry_count
                .checked_add(1)
                .ok_or(BundleError::Limit("file count"))?;
            if entry_count > limits.max_files {
                return Err(BundleError::Limit("archive entry count"));
            }
            let relative = directory.join(entry.file_name());
            let name = relative
                .to_str()
                .ok_or_else(|| BundleError::UnsafePath(relative.display().to_string()))?
                .replace(std::path::MAIN_SEPARATOR, "/");
            if excluded_paths.contains(&name.as_str()) {
                continue;
            }
            let metadata = fs::symlink_metadata(entry.path())?;
            if metadata.is_symlink() || (!metadata.is_dir() && !metadata.is_file()) {
                return Err(BundleError::UnsafePath(name));
            }
            files.register_path(&name, metadata.is_dir())?;
            if metadata.is_dir() {
                pending.push(relative);
                continue;
            }
            let executable = local_executable(&metadata, &name)?;
            let content = read_bounded(
                fs::File::open(entry.path())?,
                entry_size_limit(&files, limits)?,
            )?;
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
            let key = prefix.nfd().default_case_fold().nfd().collect::<String>();
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
        let instructions = String::from_utf8(self.contents[skill_path].clone())
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

fn entry_size_limit(files: &BundleFiles, limits: &BundleLimits) -> Result<u64, BundleError> {
    limits
        .max_expanded_bytes
        .checked_sub(files.extracted_bytes)
        .ok_or(BundleError::Limit("expanded bytes"))
}

fn read_bounded(reader: impl Read, limit: u64) -> Result<Vec<u8>, BundleError> {
    let mut bytes = Vec::new();
    reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(BundleError::Limit("expanded bytes"));
    }
    Ok(bytes)
}

#[cfg(unix)]
fn local_executable(metadata: &fs::Metadata, path: &str) -> Result<bool, BundleError> {
    use std::os::unix::fs::MetadataExt;
    if metadata.nlink() != 1 {
        return Err(BundleError::UnsafePath(path.into()));
    }
    Ok(metadata.mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn local_executable(_metadata: &fs::Metadata, _path: &str) -> Result<bool, BundleError> {
    Ok(false)
}
