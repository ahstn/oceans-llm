use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, bail};
use gateway_skills::{
    BundleLimits, ValidatedBundle, inspect_archive, validate_file_path, validate_name,
    validate_namespace,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const INSTALL_RECORD: &str = ".oceans-skill-lock.json";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallRecord {
    pub namespace: String,
    pub skill_id: Uuid,
    pub version: u32,
    pub sha256: String,
}

pub fn verify_archive(
    bytes: &[u8],
    expected_sha256: &str,
    limits: &BundleLimits,
) -> anyhow::Result<ValidatedBundle> {
    let actual = format!("{:x}", Sha256::digest(bytes));
    if actual != expected_sha256 {
        bail!("archive SHA-256 does not match the authenticated version metadata");
    }
    inspect_archive(bytes, limits).context("downloaded skill archive is invalid")
}

pub fn read_install_record(directory: &Path) -> anyhow::Result<Option<InstallRecord>> {
    let path = directory.join(INSTALL_RECORD);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("could not inspect installer record"),
    };
    if !metadata.is_file() || metadata.len() > 4096 {
        bail!("installer record must be a regular JSON file no larger than 4096 bytes");
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?.take(4097).read_to_end(&mut bytes)?;
    if bytes.len() > 4096 {
        bail!("installer record exceeds 4096 bytes");
    }
    let record: InstallRecord =
        serde_json::from_slice(&bytes).context("installer record is invalid")?;
    validate_namespace(&record.namespace)?;
    if record.version == 0
        || record.sha256.len() != 64
        || !record
            .sha256
            .bytes()
            .all(|value| value.is_ascii_digit() || matches!(value, b'a'..=b'f'))
    {
        bail!("installer record has an invalid version or SHA-256");
    }
    Ok(Some(record))
}

pub fn ensure_no_installer_record(bundle: &ValidatedBundle) -> anyhow::Result<()> {
    if bundle.contents.keys().any(|path| {
        path.split('/')
            .next()
            .is_some_and(|component| component.eq_ignore_ascii_case(INSTALL_RECORD))
    }) {
        bail!("skill contains reserved installer file {INSTALL_RECORD}");
    }
    Ok(())
}

pub fn save_download(bytes: &[u8], output: &Path) -> anyhow::Result<()> {
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).context("could not create download directory")?;
    let mut pending = tempfile::NamedTempFile::new_in(parent)?;
    pending.write_all(bytes)?;
    pending.as_file().sync_all()?;
    pending
        .persist_noclobber(output)
        .map_err(|error| error.error)
        .with_context(|| {
            format!(
                "could not save {}; existing files are not replaced",
                output.display()
            )
        })?;
    Ok(())
}

pub fn install_bundle(
    bundle: &ValidatedBundle,
    record: &InstallRecord,
    directory: &Path,
    replace: bool,
) -> anyhow::Result<PathBuf> {
    validate_name(&bundle.manifest.name)?;
    ensure_no_installer_record(bundle)?;
    // Validate every path before creating a staging directory or changing an existing install.
    for path in bundle.contents.keys() {
        validate_file_path(path)?;
    }
    fs::create_dir_all(directory).context("could not create skill installation directory")?;
    let directory = directory.canonicalize()?;
    // Cooperating installers must not replace each other's directories during the swap.
    let install_lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join(".oceans-install.lock"))?;
    install_lock
        .lock()
        .context("could not lock skill installation directory")?;
    let destination = directory.join(&bundle.manifest.name);
    let exists = check_destination(&destination, replace)?;
    let staging = tempfile::Builder::new()
        .prefix(".oceans-stage-")
        .tempdir_in(&directory)?;
    let staged_skill = staging.path().join("skill");
    write_staged_skill(bundle, record, &staged_skill)?;
    let previous = staging.path().join("previous");
    if exists {
        fs::rename(&destination, &previous).context("could not stage the previous installation")?;
    }
    if let Err(error) = fs::rename(&staged_skill, &destination) {
        if exists && let Err(rollback_error) = fs::rename(&previous, &destination) {
            let recovery = staging.keep().join("previous");
            bail!(
                "installation failed: {error}; restore failed: {rollback_error}; previous files remain at {}",
                recovery.display()
            );
        }
        return Err(error).context("could not install skill; previous installation was preserved");
    }
    Ok(destination)
}

fn check_destination(destination: &Path, replace: bool) -> anyhow::Result<bool> {
    match fs::symlink_metadata(destination) {
        Ok(metadata) => {
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                bail!(
                    "installation target {} is not a regular directory",
                    destination.display()
                );
            }
            if !replace {
                bail!(
                    "{} already exists; use --replace to replace this skill and any local edits",
                    destination.display()
                );
            }
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).context("could not inspect installation target"),
    }
}

fn write_staged_skill(
    bundle: &ValidatedBundle,
    record: &InstallRecord,
    destination: &Path,
) -> anyhow::Result<()> {
    fs::create_dir(destination)?;
    for (path, bytes) in &bundle.contents {
        let target = destination.join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&target, bytes).with_context(|| format!("could not stage skill file {path}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = if bundle.executable_files.contains(path) {
                0o755
            } else {
                0o644
            };
            fs::set_permissions(&target, fs::Permissions::from_mode(mode))?;
        }
    }
    fs::write(
        destination.join(INSTALL_RECORD),
        serde_json::to_vec_pretty(record)?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use gateway_skills::SkillManifest;
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;

    fn bundle() -> ValidatedBundle {
        ValidatedBundle {
            archive: Vec::new(),
            sha256: String::new(),
            manifest: SkillManifest {
                name: "review".into(),
                description: "Review code".into(),
                license: None,
                compatibility: None,
                metadata: BTreeMap::new(),
                allowed_tools: None,
                extra: BTreeMap::new(),
            },
            instructions: "instructions".into(),
            files: Vec::new(),
            contents: BTreeMap::from([("SKILL.md".into(), b"instructions".to_vec())]),
            executable_files: BTreeSet::new(),
            extracted_bytes: 12,
        }
    }

    fn record() -> InstallRecord {
        InstallRecord {
            namespace: "alice".into(),
            skill_id: Uuid::new_v4(),
            version: 1,
            sha256: "a".repeat(64),
        }
    }

    #[test]
    fn existing_install_requires_explicit_replacement_and_preserves_siblings() {
        let directory = tempfile::tempdir().unwrap();
        let target = install_bundle(&bundle(), &record(), directory.path(), false).unwrap();
        fs::write(target.join("local.txt"), "local edit").unwrap();
        fs::create_dir(directory.path().join("other-skill")).unwrap();
        let mut other_owner = record();
        other_owner.namespace = "bob".into();
        assert!(install_bundle(&bundle(), &other_owner, directory.path(), false).is_err());
        assert_eq!(
            fs::read_to_string(target.join("local.txt")).unwrap(),
            "local edit"
        );
        install_bundle(&bundle(), &other_owner, directory.path(), true).unwrap();
        assert!(!target.join("local.txt").exists());
        assert!(directory.path().join("other-skill").is_dir());
        let saved: InstallRecord =
            serde_json::from_slice(&fs::read(target.join(INSTALL_RECORD)).unwrap()).unwrap();
        assert_eq!(saved.namespace, "bob");
    }

    #[test]
    fn traversal_and_staging_failure_leave_existing_install_intact() {
        let directory = tempfile::tempdir().unwrap();
        let target = install_bundle(&bundle(), &record(), directory.path(), false).unwrap();
        let mut invalid = bundle();
        invalid.contents.insert("../escape".into(), b"bad".to_vec());
        assert!(install_bundle(&invalid, &record(), directory.path(), true).is_err());
        assert!(!directory.path().join("escape").exists());
        let mut conflicting = bundle();
        conflicting
            .contents
            .insert("references".into(), b"file".to_vec());
        conflicting
            .contents
            .insert("references/notes.md".into(), b"nested file".to_vec());
        assert!(install_bundle(&conflicting, &record(), directory.path(), true).is_err());
        assert_eq!(fs::read(target.join("SKILL.md")).unwrap(), b"instructions");
        assert!(!target.join("references").exists());
    }

    #[test]
    fn download_never_overwrites_an_existing_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("skill.zip");
        save_download(b"first", &path).unwrap();
        assert!(save_download(b"second", &path).is_err());
        assert_eq!(fs::read(path).unwrap(), b"first");
    }

    #[test]
    fn digest_mismatch_is_rejected_before_parsing() {
        let error =
            verify_archive(b"not a zip", &"a".repeat(64), &BundleLimits::default()).unwrap_err();
        assert!(error.to_string().contains("SHA-256"));
    }

    #[test]
    fn reserved_install_record_is_rejected_case_insensitively() {
        let directory = tempfile::tempdir().unwrap();
        for path in [".OCEANS-SKILL-LOCK.JSON", ".Oceans-Skill-Lock.Json/nested"] {
            let mut conflicting = bundle();
            conflicting
                .contents
                .insert(path.into(), b"content".to_vec());
            assert!(install_bundle(&conflicting, &record(), directory.path(), false).is_err());
            assert!(!directory.path().join("review").exists());
        }
    }

    #[cfg(unix)]
    #[test]
    fn install_does_not_follow_an_existing_skill_symlink() {
        let directory = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(external.path(), directory.path().join("review")).unwrap();
        assert!(install_bundle(&bundle(), &record(), directory.path(), true).is_err());
        assert_eq!(fs::read_dir(external.path()).unwrap().count(), 0);
    }
}
