use std::{
    fs,
    io::{Cursor, Write},
    path::PathBuf,
};

use gateway_skills::{
    BundleError, BundleLimits, INSTALL_RECORD, MAX_INSTRUCTIONS_BYTES, inspect_archive,
    pack_directory, pack_directory_excluding,
};
use zip::{ZipWriter, write::SimpleFileOptions};

const SKILL: &[u8] =
    b"---\nname: review\ndescription: Review code changes.\n---\nRun scripts/check.sh.\n";

fn archive(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        writer
            .start_file(*name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

#[test]
fn root_and_wrapped_uploads_have_the_same_canonical_digest() {
    let root = archive(&[
        ("SKILL.md", SKILL),
        ("references/checklist.md", b"Check tests."),
    ]);
    let wrapped = archive(&[
        ("review/references/checklist.md", b"Check tests."),
        ("review/SKILL.md", SKILL),
    ]);
    let limits = BundleLimits::default();
    let first = inspect_archive(&root, &limits).unwrap();
    let second = inspect_archive(&wrapped, &limits).unwrap();
    let roundtrip = inspect_archive(&first.archive, &limits).unwrap();
    assert_eq!(first.archive, second.archive);
    assert_eq!(first.sha256, roundtrip.sha256);
    assert_eq!(first.sha256.len(), 64);
    assert_eq!(first.contents["SKILL.md"], SKILL);
    assert_eq!(first.files.len(), 2);
    assert_eq!(first.extracted_bytes, SKILL.len() as u64 + 12);
}

#[test]
fn manifest_wrapper_must_match_and_contain_all_paths() {
    for files in [
        vec![("other/SKILL.md", SKILL)],
        vec![("review/SKILL.md", SKILL), ("other/file", b"file" as &[u8])],
        vec![("nested/review/SKILL.md", SKILL)],
        vec![("SKILL.md", SKILL), ("other/SKILL.md", SKILL)],
    ] {
        assert!(inspect_archive(&archive(&files), &BundleLimits::default()).is_err());
    }
}

#[test]
fn canonical_wrapper_obeys_portable_path_limits() {
    for name in ["con", "aux", "com1"] {
        let skill = format!("---\nname: {name}\ndescription: Review code.\n---\nInstructions.\n");
        let bytes = archive(&[("SKILL.md", skill.as_bytes())]);
        assert!(matches!(
            inspect_archive(&bytes, &BundleLimits::default()),
            Err(BundleError::UnsafePath(_))
        ));
    }
    let deep_path = vec!["a"; 64].join("/");
    let long_path = vec!["a".repeat(254); 4].join("/");
    for path in [&deep_path, &long_path] {
        let bytes = archive(&[("SKILL.md", SKILL), (path, b"file")]);
        assert!(inspect_archive(&bytes, &BundleLimits::default()).is_err());
    }
}

#[test]
fn rejects_unsafe_paths() {
    for path in [
        "../escape",
        "a/../escape",
        "/absolute",
        "C:/drive",
        "a\\b",
        "a//b",
        "a/./b",
        "CON.txt",
        "COM¹.txt",
        "LPT²",
        "assets/NUL",
        "a/file.",
        "a/file ",
        "a:b",
        "a\0b",
    ] {
        let result = inspect_archive(
            &archive(&[("SKILL.md", SKILL), (path, b"bad")]),
            &BundleLimits::default(),
        );
        assert!(result.is_err(), "accepted {path:?}");
    }
}

#[test]
fn rejects_case_unicode_and_ancestor_collisions() {
    for (first, second) in [
        ("a.txt", "A.txt"),
        ("Assets/a", "assets/b"),
        ("a", "a/file"),
        ("a/file", "a"),
        ("café", "cafe\u{301}"),
        ("Σ", "ς"),
        ("σ", "ς"),
        ("ß", "ss"),
        ("ﬀ", "ff"),
    ] {
        let bytes = archive(&[("SKILL.md", SKILL), (first, b"one"), (second, b"two")]);
        assert!(
            matches!(
                inspect_archive(&bytes, &BundleLimits::default()),
                Err(BundleError::DuplicatePath(_))
            ),
            "accepted {first:?} and {second:?}"
        );
    }
}

#[test]
fn rejects_identical_names_before_zip_library_deduplication() {
    let mut bytes = archive(&[("SKILL.md", SKILL), ("a.txt", b"one"), ("b.txt", b"two")]);
    let positions: Vec<_> = bytes
        .windows(5)
        .enumerate()
        .filter_map(|(index, value)| (value == b"b.txt").then_some(index))
        .collect();
    for position in positions {
        bytes[position] = b'a';
    }
    assert!(matches!(
        inspect_archive(&bytes, &BundleLimits::default()),
        Err(BundleError::DuplicatePath(_))
    ));
}

#[test]
fn rejects_symbolic_links_and_unix_link_extra_fields() {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file("SKILL.md", SimpleFileOptions::default())
        .unwrap();
    writer.write_all(SKILL).unwrap();
    writer
        .add_symlink("escape", "../target", SimpleFileOptions::default())
        .unwrap();
    let bytes = writer.finish().unwrap().into_inner();
    assert!(matches!(
        inspect_archive(&bytes, &BundleLimits::default()),
        Err(BundleError::UnsafePath(_))
    ));

    // Insert a PKWARE Unix link field into the central directory of a plain ZIP.
    let mut bytes = archive(&[("SKILL.md", SKILL)]);
    let central = bytes
        .windows(4)
        .position(|value| value == b"PK\x01\x02")
        .unwrap();
    let extra_start = central + 46 + "SKILL.md".len();
    let old_extra_size = u16::from_le_bytes(bytes[central + 30..central + 32].try_into().unwrap());
    bytes[central + 30..central + 32].copy_from_slice(&(old_extra_size + 4).to_le_bytes());
    bytes.splice(extra_start..extra_start, [0x0d, 0x00, 0x00, 0x00]);
    let end = bytes.len() - 22;
    let old_size = u32::from_le_bytes(bytes[end + 12..end + 16].try_into().unwrap());
    bytes[end + 12..end + 16].copy_from_slice(&(old_size + 4).to_le_bytes());
    assert!(matches!(
        inspect_archive(&bytes, &BundleLimits::default()),
        Err(BundleError::Archive(_))
    ));
}

#[test]
fn enforces_compressed_expanded_and_entry_limits() {
    let bomb = vec![b'a'; 512 * 1024];
    let bytes = archive(&[("SKILL.md", SKILL), ("large.txt", &bomb)]);
    for limits in [
        BundleLimits {
            max_archive_bytes: 8,
            ..Default::default()
        },
        BundleLimits {
            max_expanded_bytes: 1024,
            ..Default::default()
        },
        BundleLimits {
            max_files: 1,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            inspect_archive(&bytes, &limits),
            Err(BundleError::Limit(_))
        ));
    }
    // A forged advertised length does not bypass the actual expanded read limit.
    let mut forged = bytes;
    let central_positions: Vec<_> = forged
        .windows(4)
        .enumerate()
        .filter_map(|(i, value)| (value == b"PK\x01\x02").then_some(i))
        .collect();
    let central = central_positions[1];
    forged[central + 24..central + 28].copy_from_slice(&1u32.to_le_bytes());
    assert!(
        inspect_archive(
            &forged,
            &BundleLimits {
                max_expanded_bytes: 1024,
                ..Default::default()
            }
        )
        .is_err()
    );
}

#[test]
fn manifest_fields_are_strict_and_extensions_survive() {
    let yaml = b"---\nname: review\ndescription: Review code.\nlicense: MIT\ncompatibility: Requires git\nmetadata:\n  version: '1'\nallowed-tools: Read Bash(git:*)\nx-ui:\n  icon: code\n---\nInstructions.\n";
    let bundle =
        inspect_archive(&archive(&[("SKILL.md", yaml)]), &BundleLimits::default()).unwrap();
    assert_eq!(bundle.manifest.metadata["version"], "1");
    assert_eq!(bundle.manifest.extra["x-ui"]["icon"], "code");
    for bad in [
        "name: Bad",
        "name: bad--name",
        "name: 123",
        "description: ''",
        "description: true",
        "metadata: {version: 1}",
        "metadata: {1: text}",
        "allowed-tools: [Read]",
        "license: null",
        "compatibility: ''",
    ] {
        let header = if bad.starts_with("name:") {
            "description: Review code.\n"
        } else if bad.starts_with("description:") {
            "name: review\n"
        } else {
            "name: review\ndescription: Review code.\n"
        };
        let skill = format!("---\n{header}{bad}\n---\nInstructions.\n");
        assert!(
            inspect_archive(
                &archive(&[("SKILL.md", skill.as_bytes())]),
                &BundleLimits::default()
            )
            .is_err(),
            "accepted {bad}"
        );
    }
}

#[test]
fn instruction_limit_applies_to_root_wrapped_and_local_bundles() {
    let mut instructions = SKILL.to_vec();
    instructions.resize(MAX_INSTRUCTIONS_BYTES, b'a');
    let limits = BundleLimits::default();
    let directory = TestDirectory::new();
    for size in [MAX_INSTRUCTIONS_BYTES, MAX_INSTRUCTIONS_BYTES + 1] {
        instructions.resize(size, b'a');
        for path in ["SKILL.md", "review/SKILL.md"] {
            let result = inspect_archive(&archive(&[(path, &instructions)]), &limits);
            if size == MAX_INSTRUCTIONS_BYTES {
                assert_eq!(result.unwrap().instructions.len(), size);
            } else {
                assert!(matches!(
                    result,
                    Err(BundleError::Limit("SKILL.md bytes (256 KiB)"))
                ));
            }
        }
        fs::write(directory.0.join("SKILL.md"), &instructions).unwrap();
        assert_eq!(
            pack_directory(&directory.0, &limits).is_ok(),
            size == MAX_INSTRUCTIONS_BYTES
        );
    }
}

#[test]
fn rejects_raw_and_yaml_escaped_nul_in_instructions() {
    for instructions in [
        "---\nname: review\ndescription: Review code.\n---\nBody\0text",
        "---\nname: review\ndescription: \"Review\\0code\"\n---\nBody",
        "---\nname: review\ndescription: Review code.\nmetadata: {author: \"a\\u0000b\"}\n---\nBody",
        "---\nname: review\ndescription: Review code.\nx-options: {\"a\\0b\": value}\n---\nBody",
    ] {
        let result = inspect_archive(
            &archive(&[("SKILL.md", instructions.as_bytes())]),
            &BundleLimits::default(),
        );
        assert!(matches!(result, Err(BundleError::Manifest(message)) if message.contains("NUL")));
    }
}

#[test]
fn rejects_reserved_installer_records_in_root_and_wrapped_archives() {
    for prefix in ["", "review/"] {
        for path in [
            INSTALL_RECORD,
            ".OCEANS-SKILL-LOCK.JSON",
            ".Oceans-Skill-Lock.Json/nested",
            ".oceans-ſkill-lock.json",
            ".oceans-skill-locK.json/nested",
        ] {
            let bytes = archive(&[
                (&format!("{prefix}SKILL.md"), SKILL),
                (&format!("{prefix}{path}"), b"record"),
            ]);
            assert!(matches!(
                inspect_archive(&bytes, &BundleLimits::default()),
                Err(BundleError::UnsafePath(_))
            ));
        }
    }
    let bytes = archive(&[
        ("SKILL.md", SKILL),
        ("assets/.oceans-skill-lock.json", b"asset"),
    ]);
    assert!(inspect_archive(&bytes, &BundleLimits::default()).is_ok());
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir()
            .join(format!("gateway-skills-{}", uuid::Uuid::new_v4()))
            .join("review");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("SKILL.md"), SKILL).unwrap();
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(self.0.parent().unwrap());
    }
}

#[test]
fn local_pack_accepts_dot_path_and_roundtrips() {
    let directory = TestDirectory::new();
    fs::create_dir(directory.0.join("scripts")).unwrap();
    fs::write(directory.0.join("scripts/check.sh"), b"#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            directory.0.join("scripts/check.sh"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    let bundle = pack_directory(&directory.0.join("."), &BundleLimits::default()).unwrap();
    let roundtrip = inspect_archive(&bundle.archive, &BundleLimits::default()).unwrap();
    assert_eq!(bundle.sha256, roundtrip.sha256);
    #[cfg(unix)]
    assert!(roundtrip.executable_files.contains("scripts/check.sh"));
}

#[test]
fn local_pack_can_omit_only_exact_client_metadata_paths() {
    let directory = TestDirectory::new();
    fs::write(
        directory.0.join(".oceans-skill-lock.json"),
        b"client metadata",
    )
    .unwrap();
    fs::create_dir(directory.0.join("assets")).unwrap();
    fs::write(
        directory.0.join("assets/.oceans-skill-lock.json"),
        b"skill asset",
    )
    .unwrap();
    let bundle = pack_directory_excluding(
        &directory.0,
        &BundleLimits::default(),
        &[".oceans-skill-lock.json"],
    )
    .unwrap();
    assert!(!bundle.contents.contains_key(".oceans-skill-lock.json"));
    assert_eq!(
        bundle.contents["assets/.oceans-skill-lock.json"],
        b"skill asset"
    );
    assert!(
        pack_directory_excluding(&directory.0, &BundleLimits::default(), &["../file"]).is_err()
    );
}

#[test]
fn excluded_installer_record_does_not_consume_entry_limit() {
    let directory = TestDirectory::new();
    fs::write(directory.0.join(INSTALL_RECORD), b"record").unwrap();
    let limits = BundleLimits {
        max_files: 1,
        ..Default::default()
    };
    let bundle = pack_directory_excluding(&directory.0, &limits, &[INSTALL_RECORD]).unwrap();
    assert_eq!(bundle.files.len(), 1);
    fs::write(directory.0.join("extra.txt"), b"file").unwrap();
    assert!(matches!(
        pack_directory_excluding(&directory.0, &limits, &[INSTALL_RECORD]),
        Err(BundleError::Limit("archive entry count"))
    ));
}

#[cfg(unix)]
#[test]
fn local_pack_rejects_links() {
    let directory = TestDirectory::new();
    std::os::unix::fs::symlink("SKILL.md", directory.0.join("link")).unwrap();
    assert!(pack_directory(&directory.0, &BundleLimits::default()).is_err());
    fs::remove_file(directory.0.join("link")).unwrap();
    fs::hard_link(directory.0.join("SKILL.md"), directory.0.join("hardlink")).unwrap();
    assert!(pack_directory(&directory.0, &BundleLimits::default()).is_err());
    let alias = directory.0.parent().unwrap().join("alias");
    std::os::unix::fs::symlink(&directory.0, &alias).unwrap();
    assert!(pack_directory(&alias, &BundleLimits::default()).is_err());
}
