use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use super::*;

#[derive(Default)]
struct CleanupStore {
    deleted: AtomicUsize,
}

#[async_trait::async_trait]
impl SkillObjectStore for CleanupStore {
    async fn put(&self, _key: &str, _bytes: &[u8]) -> Result<(), StoreError> {
        unreachable!("cleanup does not upload")
    }

    async fn get(&self, _key: &str, _max_bytes: u64) -> Result<Vec<u8>, StoreError> {
        unreachable!("cleanup does not download")
    }

    async fn delete(&self, _key: &str) -> Result<(), StoreError> {
        self.deleted.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

#[tokio::test]
async fn metadata_rejection_cleans_up_but_uncertain_commit_preserves_archive() {
    let objects = CleanupStore::default();
    for error in [
        StoreError::Conflict("duplicate".into()),
        StoreError::NotFound("owner".into()),
    ] {
        assert!(
            finish_upload(&objects, "upload.zip", Err(error))
                .await
                .is_err()
        );
    }
    assert_eq!(objects.deleted.load(Ordering::Relaxed), 2);
    for error in [
        StoreError::Query("connection lost during commit".into()),
        StoreError::Unavailable("database unavailable".into()),
        StoreError::Unexpected("unknown outcome".into()),
    ] {
        assert!(
            finish_upload(&objects, "upload.zip", Err(error))
                .await
                .is_err()
        );
    }
    assert_eq!(objects.deleted.load(Ordering::Relaxed), 2);
}

struct PreviewRepository {
    version: SkillVersionRecord,
    reads: AtomicUsize,
    upload_response: Option<SkillUploadResponse>,
    committed: AtomicBool,
}

#[async_trait::async_trait]
impl SkillRepository for PreviewRepository {
    async fn claim_skill_namespace(
        &self,
        _user_id: Uuid,
        _handle: &str,
        _now: OffsetDateTime,
    ) -> Result<SkillNamespaceRecord, StoreError> {
        unreachable!("preview does not claim namespaces")
    }

    async fn get_skill_namespace(
        &self,
        user_id: Uuid,
    ) -> Result<Option<SkillNamespaceRecord>, StoreError> {
        assert!(!self.committed.load(Ordering::Relaxed));
        Ok(Some(SkillNamespaceRecord {
            user_id,
            handle: "owner".into(),
            created_at: self.version.created_at,
        }))
    }

    async fn list_skills(&self, _query: &SkillListQuery) -> Result<Vec<SkillRecord>, StoreError> {
        unreachable!("preview does not list skills")
    }

    async fn get_skill(&self, _skill_id: Uuid) -> Result<Option<SkillRecord>, StoreError> {
        if self.committed.load(Ordering::Relaxed) {
            return Err(StoreError::Unavailable("post-commit read failed".into()));
        }
        Ok(self.upload_response.as_ref().map(|response| {
            let mut skill = response.detail.skill.clone();
            // Simulate a concurrent append/default change after the ownership read.
            skill.default_version = 1;
            skill.latest_version = 1;
            skill
        }))
    }

    async fn get_skill_by_name(
        &self,
        _namespace: &str,
        _name: &str,
    ) -> Result<Option<SkillRecord>, StoreError> {
        unreachable!("preview reads the version directly")
    }

    async fn create_skill(
        &self,
        _owner_user_id: Uuid,
        _name: &str,
        _metadata: &SkillVersionMetadata,
        _now: OffsetDateTime,
    ) -> Result<SkillUploadResponse, StoreError> {
        self.committed.store(true, Ordering::Relaxed);
        Ok(self.upload_response.clone().expect("configured upload"))
    }

    async fn append_skill_version(
        &self,
        _owner_user_id: Uuid,
        _skill_id: Uuid,
        _metadata: &SkillVersionMetadata,
        _now: OffsetDateTime,
    ) -> Result<SkillUploadResponse, StoreError> {
        self.committed.store(true, Ordering::Relaxed);
        Ok(self.upload_response.clone().expect("configured upload"))
    }

    async fn list_skill_versions(
        &self,
        _skill_id: Uuid,
    ) -> Result<Vec<SkillVersionSummary>, StoreError> {
        Err(StoreError::Unavailable("version list read failed".into()))
    }

    async fn get_skill_version(
        &self,
        skill_id: Uuid,
        version: u32,
    ) -> Result<Option<SkillVersionRecord>, StoreError> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        Ok(
            (skill_id == self.version.skill_id && version == self.version.version)
                .then(|| self.version.clone()),
        )
    }

    async fn set_skill_default_version(
        &self,
        _owner_user_id: Uuid,
        _skill_id: Uuid,
        _version: u32,
        _now: OffsetDateTime,
    ) -> Result<SkillRecord, StoreError> {
        unreachable!("preview does not change the default")
    }
}

struct PreviewObjects {
    bytes: Vec<u8>,
    read_limit: AtomicU64,
    reads: AtomicUsize,
    writes: AtomicUsize,
}

#[async_trait::async_trait]
impl SkillObjectStore for PreviewObjects {
    async fn put(&self, _key: &str, _bytes: &[u8]) -> Result<(), StoreError> {
        self.writes.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    async fn get(&self, _key: &str, max_bytes: u64) -> Result<Vec<u8>, StoreError> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.read_limit.store(max_bytes, Ordering::Relaxed);
        Ok(self.bytes.clone())
    }

    async fn delete(&self, _key: &str) -> Result<(), StoreError> {
        unreachable!("preview does not delete")
    }
}

fn preview_fixture() -> (PreviewRepository, PreviewObjects) {
    preview_fixture_with_reference("Stored reference.\n")
}

fn preview_fixture_with_reference(reference: &str) -> (PreviewRepository, PreviewObjects) {
    let directory = std::env::temp_dir().join(format!("skill-preview-{}", Uuid::new_v4()));
    let skill = directory.join("preview-skill");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: preview-skill\ndescription: Test stored previews.\n---\n# Preview\n",
    )
    .unwrap();
    std::fs::write(skill.join("reference.txt"), reference).unwrap();
    let bundle = gateway_skills::pack_directory(&skill, &BundleLimits::default());
    std::fs::remove_dir_all(directory).unwrap();
    let bundle = bundle.unwrap();
    let version = SkillVersionRecord {
        skill_id: Uuid::new_v4(),
        version: 1,
        metadata: version_metadata(&bundle).unwrap(),
        created_at: OffsetDateTime::now_utc(),
    };
    (
        PreviewRepository {
            version,
            reads: AtomicUsize::new(0),
            upload_response: None,
            committed: AtomicBool::new(false),
        },
        PreviewObjects {
            bytes: bundle.archive,
            read_limit: AtomicU64::new(0),
            reads: AtomicUsize::new(0),
            writes: AtomicUsize::new(0),
        },
    )
}

#[tokio::test]
async fn stored_file_preview_survives_lower_upload_limits() {
    let mut failures = Vec::new();
    for (label, limits) in [
        (
            "archive bytes",
            BundleLimits {
                max_archive_bytes: 1,
                ..BundleLimits::default()
            },
        ),
        (
            "expanded bytes",
            BundleLimits {
                max_expanded_bytes: 1,
                ..BundleLimits::default()
            },
        ),
        (
            "file count",
            BundleLimits {
                max_files: 1,
                ..BundleLimits::default()
            },
        ),
        (
            "all limits",
            BundleLimits {
                max_archive_bytes: 1,
                max_expanded_bytes: 1,
                max_files: 1,
            },
        ),
    ] {
        let (repository, objects) = preview_fixture();
        let id = repository.version.skill_id;
        let service = SkillService::new(Arc::new(repository), Arc::new(objects), limits);
        match service.file(id, 1, "reference.txt").await {
            Ok(file) => {
                assert_eq!(file.path, "reference.txt");
                assert_eq!(file.content, "Stored reference.\n");
                assert_eq!(service.repository.reads.load(Ordering::Relaxed), 1);
                assert_eq!(
                    service.objects.read_limit.load(Ordering::Relaxed),
                    service.repository.version.metadata.archive_bytes
                );
            }
            Err(error) => failures.push(format!("{label}: {error}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[tokio::test]
async fn stored_archive_download_uses_recorded_size_after_upload_limits_change() {
    let (repository, objects) = preview_fixture();
    let id = repository.version.skill_id;
    let expected_bytes = objects.bytes.clone();
    let expected_digest = repository.version.metadata.sha256.clone();
    let service = SkillService::new(
        Arc::new(repository),
        Arc::new(objects),
        BundleLimits {
            max_archive_bytes: 1,
            max_expanded_bytes: 1,
            max_files: 1,
        },
    );
    let (bytes, digest) = service.archive(id, 1).await.unwrap();
    assert_eq!(bytes, expected_bytes);
    assert_eq!(digest, expected_digest);
    assert_eq!(
        service.objects.read_limit.load(Ordering::Relaxed),
        expected_bytes.len() as u64
    );
    assert_eq!(service.repository.reads.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn stored_file_preview_enforces_recorded_expansion_bounds() {
    for limit in ["expanded bytes", "file count"] {
        let (mut repository, objects) = preview_fixture();
        let id = repository.version.skill_id;
        match limit {
            "expanded bytes" => repository.version.metadata.extracted_bytes = 1,
            "file count" => repository.version.metadata.file_count = 1,
            _ => unreachable!(),
        }
        let service = SkillService::new(
            Arc::new(repository),
            Arc::new(objects),
            BundleLimits::default(),
        );
        let result = service.file(id, 1, "reference.txt").await;
        assert!(
            matches!(result, Err(GatewayError::InvalidRequest(_))),
            "{limit}"
        );
    }
}

#[tokio::test]
async fn missing_file_preview_does_not_read_object_storage() {
    let (repository, objects) = preview_fixture();
    let id = repository.version.skill_id;
    let service = SkillService::new(
        Arc::new(repository),
        Arc::new(objects),
        BundleLimits::default(),
    );
    let result = service.file(id, 1, "missing.txt").await;
    assert!(matches!(
        result,
        Err(GatewayError::Store(StoreError::NotFound(_)))
    ));
    assert_eq!(service.objects.reads.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn unsafe_preview_path_does_not_read_metadata_or_objects() {
    let (repository, objects) = preview_fixture();
    let service = SkillService::new(
        Arc::new(repository),
        Arc::new(objects),
        BundleLimits::default(),
    );
    let result = service.file(Uuid::new_v4(), 1, "../secret.txt").await;
    assert!(matches!(result, Err(GatewayError::InvalidRequest(_))));
    assert_eq!(service.repository.reads.load(Ordering::Relaxed), 0);
    assert_eq!(service.objects.reads.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn stored_file_preview_rejects_corrupt_archive() {
    for change_length in [false, true] {
        let (repository, mut objects) = preview_fixture();
        let id = repository.version.skill_id;
        if change_length {
            objects.bytes.push(0);
        } else {
            objects.bytes[0] ^= 1;
        }
        let service = SkillService::new(
            Arc::new(repository),
            Arc::new(objects),
            BundleLimits::default(),
        );
        let result = service.file(id, 1, "reference.txt").await;
        assert!(matches!(
            result,
            Err(GatewayError::Store(StoreError::Unavailable(_)))
        ));
    }
}

#[tokio::test]
async fn stored_file_preview_retains_archive_validation_after_integrity_check() {
    let (mut repository, mut objects) = preview_fixture();
    let id = repository.version.skill_id;
    let path = b"preview-skill/SKILL.md";
    for start in 0..objects.bytes.len() - path.len() {
        if &objects.bytes[start..start + path.len()] == path {
            objects.bytes[start + "preview-skill/".len()..start + path.len()]
                .copy_from_slice(b"../LL.md");
        }
    }
    repository.version.metadata.sha256 = format!("{:x}", Sha256::digest(&objects.bytes));
    let service = SkillService::new(
        Arc::new(repository),
        Arc::new(objects),
        BundleLimits::default(),
    );
    let result = service.file(id, 1, "reference.txt").await;
    assert!(
        matches!(result, Err(GatewayError::InvalidRequest(message)) if message.contains("unsafe skill path"))
    );
}

#[tokio::test]
async fn oversized_file_preview_is_rejected_before_object_read() {
    let (repository, objects) =
        preview_fixture_with_reference(&"\u{0001}".repeat(MAX_FILE_PREVIEW_BYTES + 1));
    let id = repository.version.skill_id;
    let service = SkillService::new(
        Arc::new(repository),
        Arc::new(objects),
        BundleLimits::default(),
    );
    let result = service.file(id, 1, "reference.txt").await;
    assert!(
        matches!(result, Err(GatewayError::InvalidRequest(message)) if message.contains("preview limit; download the archive instead"))
    );
    assert_eq!(service.objects.reads.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn oversized_file_preview_is_rejected_even_if_recorded_file_size_is_wrong() {
    let (mut repository, objects) =
        preview_fixture_with_reference(&"\u{0001}".repeat(MAX_FILE_PREVIEW_BYTES + 1));
    let id = repository.version.skill_id;
    repository
        .version
        .metadata
        .files
        .iter_mut()
        .find(|file| file.path == "reference.txt")
        .unwrap()
        .size = 1;
    let service = SkillService::new(
        Arc::new(repository),
        Arc::new(objects),
        BundleLimits::default(),
    );
    let result = service.file(id, 1, "reference.txt").await;
    assert!(
        matches!(result, Err(GatewayError::InvalidRequest(message)) if message.contains("preview limit; download the archive instead"))
    );
    assert_eq!(service.objects.reads.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn file_preview_accepts_limit_and_bounds_json_control_character_expansion() {
    let content = "\u{0001}".repeat(MAX_FILE_PREVIEW_BYTES);
    let (repository, objects) = preview_fixture_with_reference(&content);
    let id = repository.version.skill_id;
    let service = SkillService::new(
        Arc::new(repository),
        Arc::new(objects),
        BundleLimits::default(),
    );
    let file = service.file(id, 1, "reference.txt").await.unwrap();
    assert_eq!(file.content, content);
    assert!(serde_json::to_vec(&file).unwrap().len() <= MAX_FILE_PREVIEW_BYTES * 6 + 64);
}

#[tokio::test]
async fn legacy_instructions_are_bounded_without_blocking_archive_download() {
    let (mut repository, objects) = preview_fixture();
    let id = repository.version.skill_id;
    let expected_archive = objects.bytes.clone();
    repository.version.metadata.instructions = "\u{0001}".repeat(MAX_INSTRUCTIONS_BYTES + 1);
    let service = SkillService::new(
        Arc::new(repository),
        Arc::new(objects),
        BundleLimits::default(),
    );
    let result = service.version(id, 1).await;
    assert!(
        matches!(result, Err(GatewayError::InvalidRequest(message)) if message.contains("preview limit; download the archive instead"))
    );
    assert_eq!(service.objects.reads.load(Ordering::Relaxed), 0);
    assert_eq!(service.archive(id, 1).await.unwrap().0, expected_archive);
}

#[tokio::test]
async fn version_instructions_accept_limit_with_bounded_json_expansion() {
    let (mut repository, objects) = preview_fixture();
    let id = repository.version.skill_id;
    repository.version.metadata.instructions = "\u{0001}".repeat(MAX_INSTRUCTIONS_BYTES);
    let service = SkillService::new(
        Arc::new(repository),
        Arc::new(objects),
        BundleLimits::default(),
    );
    let version = service.version(id, 1).await.unwrap();
    assert_eq!(version.instructions.len(), MAX_INSTRUCTIONS_BYTES);
    assert!(serde_json::to_vec(&version).unwrap().len() <= MAX_INSTRUCTIONS_BYTES * 6 + 4096);
}

#[tokio::test]
async fn uploads_return_committed_snapshot_without_post_commit_reads() {
    for append in [false, true] {
        let (mut repository, objects) = preview_fixture();
        let id = repository.version.skill_id;
        let owner = Uuid::new_v4();
        let uploaded_version = if append { 3 } else { 1 };
        let versions: Vec<_> = (1..=uploaded_version)
            .rev()
            .map(|version| SkillVersionSummary {
                version,
                ..version_summary(&repository.version)
            })
            .collect();
        let expected = SkillUploadResponse {
            detail: SkillDetail {
                skill: SkillRecord {
                    id,
                    namespace: "owner".into(),
                    name: repository.version.metadata.manifest.name.clone(),
                    owner_user_id: owner,
                    description: "Description committed by the repository.".into(),
                    default_version: if append { 2 } else { 1 },
                    latest_version: uploaded_version,
                    created_at: repository.version.created_at,
                    updated_at: repository.version.created_at,
                },
                versions,
            },
            uploaded_version,
        };
        repository.upload_response = Some(expected.clone());
        let archive = objects.bytes.clone();
        let service = SkillService::new(
            Arc::new(repository),
            Arc::new(objects),
            BundleLimits::default(),
        );
        let actual = if append {
            service.append(owner, id, archive).await
        } else {
            service.create(owner, archive).await
        }
        .unwrap();
        assert_eq!(actual, expected);
        assert!(service.repository.committed.load(Ordering::Relaxed));
        assert_eq!(service.objects.writes.load(Ordering::Relaxed), 1);
    }
}
