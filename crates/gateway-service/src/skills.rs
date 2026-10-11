//! Authenticated callers share a catalog; only the owning user can change a skill.

use std::sync::Arc;

use gateway_core::{
    AuthError, GatewayError, SkillListQuery, SkillNamespaceRecord, SkillObjectStore, SkillRecord,
    SkillRepository, SkillVersionMetadata, SkillVersionRecord, StoreError,
};
use gateway_skills::{
    BundleLimits, MAX_INSTRUCTIONS_BYTES, SkillDetail, SkillFileContent, SkillNamespace,
    SkillSummary, SkillUploadResponse, SkillVersionDetail, SkillVersionSummary, ValidatedBundle,
    inspect_archive, validate_file_path, validate_namespace,
};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

// JSON can expand each control byte to six bytes. Keep inline text responses bounded.
const MAX_FILE_PREVIEW_BYTES: usize = 256 * 1024;

pub struct SkillService<S, O: ?Sized> {
    repository: Arc<S>,
    objects: Arc<O>,
    limits: BundleLimits,
}

impl<S, O: ?Sized> SkillService<S, O>
where
    S: SkillRepository,
    O: SkillObjectStore,
{
    pub fn new(repository: Arc<S>, objects: Arc<O>, limits: BundleLimits) -> Self {
        Self {
            repository,
            objects,
            limits,
        }
    }

    pub fn limits(&self) -> &BundleLimits {
        &self.limits
    }

    pub async fn namespace(&self, user_id: Uuid) -> Result<Option<SkillNamespace>, GatewayError> {
        Ok(self
            .repository
            .get_skill_namespace(user_id)
            .await?
            .map(namespace_view))
    }

    pub async fn claim_namespace(
        &self,
        user_id: Uuid,
        handle: &str,
    ) -> Result<SkillNamespace, GatewayError> {
        validate_namespace(handle).map_err(invalid_bundle)?;
        let namespace = self
            .repository
            .claim_skill_namespace(user_id, handle, OffsetDateTime::now_utc())
            .await?;
        Ok(namespace_view(namespace))
    }

    pub async fn list(&self, query: &SkillListQuery) -> Result<Vec<SkillSummary>, GatewayError> {
        if !(1..=100).contains(&query.limit) {
            return Err(GatewayError::InvalidRequest(
                "limit must be between 1 and 100".into(),
            ));
        }
        Ok(self.repository.list_skills(query).await?)
    }

    pub async fn detail(&self, id: Uuid) -> Result<SkillDetail, GatewayError> {
        let skill = self.require_skill(id).await?;
        let versions = self.repository.list_skill_versions(id).await?;
        Ok(SkillDetail { skill, versions })
    }

    pub async fn by_name(&self, namespace: &str, name: &str) -> Result<SkillDetail, GatewayError> {
        let record = self
            .repository
            .get_skill_by_name(namespace, name)
            .await?
            .ok_or_else(|| StoreError::NotFound("skill".into()))?;
        self.detail(record.id).await
    }

    pub async fn versions(&self, id: Uuid) -> Result<Vec<SkillVersionSummary>, GatewayError> {
        self.require_skill(id).await?;
        Ok(self.repository.list_skill_versions(id).await?)
    }

    pub async fn version(
        &self,
        id: Uuid,
        version: u32,
    ) -> Result<SkillVersionDetail, GatewayError> {
        let record = self.require_version(id, version).await?;
        if record.metadata.instructions.len() > MAX_INSTRUCTIONS_BYTES {
            return Err(GatewayError::InvalidRequest(format!(
                "skill instructions exceed the {MAX_INSTRUCTIONS_BYTES}-byte preview limit; download the archive instead"
            )));
        }
        Ok(SkillVersionDetail {
            version: version_summary(&record),
            manifest: record.metadata.manifest,
            files: record.metadata.files,
            instructions: record.metadata.instructions,
        })
    }

    pub async fn create(
        &self,
        user_id: Uuid,
        bytes: Vec<u8>,
    ) -> Result<SkillUploadResponse, GatewayError> {
        self.namespace(user_id).await?.ok_or_else(|| {
            GatewayError::InvalidRequest("claim a skill namespace before uploading".into())
        })?;
        let bundle = Self::validate(bytes, self.limits).await?;
        let metadata = version_metadata(&bundle)?;
        self.objects
            .put(&metadata.object_key, &bundle.archive)
            .await?;
        let result = self
            .repository
            .create_skill(
                user_id,
                &bundle.manifest.name,
                &metadata,
                OffsetDateTime::now_utc(),
            )
            .await;
        finish_upload(self.objects.as_ref(), &metadata.object_key, result).await
    }

    pub async fn append(
        &self,
        user_id: Uuid,
        id: Uuid,
        bytes: Vec<u8>,
    ) -> Result<SkillUploadResponse, GatewayError> {
        let skill = self.require_owner(user_id, id).await?;
        let bundle = Self::validate(bytes, self.limits).await?;
        if bundle.manifest.name != skill.name {
            return Err(GatewayError::InvalidRequest(
                "a new version must keep the skill name".into(),
            ));
        }
        let metadata = version_metadata(&bundle)?;
        self.objects
            .put(&metadata.object_key, &bundle.archive)
            .await?;
        let result = self
            .repository
            .append_skill_version(user_id, id, &metadata, OffsetDateTime::now_utc())
            .await;
        finish_upload(self.objects.as_ref(), &metadata.object_key, result).await
    }

    pub async fn set_default(
        &self,
        user_id: Uuid,
        id: Uuid,
        version: u32,
    ) -> Result<SkillDetail, GatewayError> {
        self.require_owner(user_id, id).await?;
        self.repository
            .set_skill_default_version(user_id, id, version, OffsetDateTime::now_utc())
            .await?;
        self.detail(id).await
    }

    pub async fn archive(&self, id: Uuid, version: u32) -> Result<(Vec<u8>, String), GatewayError> {
        let record = self.require_version(id, version).await?;
        let bytes = self.read_archive(&record.metadata).await?;
        Ok((bytes, record.metadata.sha256))
    }

    pub async fn file(
        &self,
        id: Uuid,
        version: u32,
        path: &str,
    ) -> Result<SkillFileContent, GatewayError> {
        validate_file_path(path).map_err(invalid_bundle)?;
        let metadata = self.require_version(id, version).await?.metadata;
        let file = metadata
            .files
            .iter()
            .find(|file| file.path == path)
            .ok_or_else(|| StoreError::NotFound("skill file".into()))?;
        check_file_preview_size(file.size)?;
        let bytes = self.read_archive(&metadata).await?;
        // Immutable versions retain their validated bounds when upload limits change.
        let limits = BundleLimits {
            max_archive_bytes: metadata.archive_bytes,
            max_expanded_bytes: metadata.extracted_bytes,
            max_files: metadata.file_count,
        };
        let mut bundle = Self::validate(bytes, limits).await?;
        let bytes = bundle
            .contents
            .remove(path)
            .ok_or_else(|| StoreError::NotFound("skill file".into()))?;
        check_file_preview_size(bytes.len() as u64)?;
        let content = String::from_utf8(bytes).map_err(|_| {
            GatewayError::InvalidRequest("binary files cannot be previewed as text".into())
        })?;
        Ok(SkillFileContent {
            path: path.to_owned(),
            content,
        })
    }

    async fn read_archive(&self, metadata: &SkillVersionMetadata) -> Result<Vec<u8>, GatewayError> {
        let bytes = self
            .objects
            .get(&metadata.object_key, metadata.archive_bytes)
            .await?;
        if bytes.len() as u64 != metadata.archive_bytes
            || format!("{:x}", Sha256::digest(&bytes)) != metadata.sha256
        {
            return Err(
                StoreError::Unavailable("skill archive failed its integrity check".into()).into(),
            );
        }
        Ok(bytes)
    }

    async fn validate(
        bytes: Vec<u8>,
        limits: BundleLimits,
    ) -> Result<ValidatedBundle, GatewayError> {
        if bytes.len() as u64 > limits.max_archive_bytes {
            return Err(GatewayError::PayloadTooLarge {
                limit_bytes: usize::try_from(limits.max_archive_bytes).unwrap_or(usize::MAX),
            });
        }
        tokio::task::spawn_blocking(move || inspect_archive(&bytes, &limits))
            .await
            .map_err(|_| GatewayError::Internal("skill validation task failed".into()))?
            .map_err(invalid_bundle)
    }

    async fn require_skill(&self, id: Uuid) -> Result<SkillRecord, GatewayError> {
        self.repository
            .get_skill(id)
            .await?
            .ok_or_else(|| StoreError::NotFound("skill".into()).into())
    }

    async fn require_owner(&self, user_id: Uuid, id: Uuid) -> Result<SkillRecord, GatewayError> {
        let record = self.require_skill(id).await?;
        if record.owner_user_id != user_id {
            return Err(AuthError::InsufficientPrivileges.into());
        }
        Ok(record)
    }

    async fn require_version(
        &self,
        id: Uuid,
        version: u32,
    ) -> Result<SkillVersionRecord, GatewayError> {
        self.repository
            .get_skill_version(id, version)
            .await?
            .ok_or_else(|| StoreError::NotFound("skill version".into()).into())
    }
}

fn check_file_preview_size(size: u64) -> Result<(), GatewayError> {
    if size > MAX_FILE_PREVIEW_BYTES as u64 {
        return Err(GatewayError::InvalidRequest(format!(
            "skill file exceeds the {MAX_FILE_PREVIEW_BYTES}-byte preview limit; download the archive instead"
        )));
    }
    Ok(())
}

async fn finish_upload<O: SkillObjectStore + ?Sized>(
    objects: &O,
    object_key: &str,
    result: Result<SkillUploadResponse, StoreError>,
) -> Result<SkillUploadResponse, GatewayError> {
    match result {
        Ok(record) => Ok(record),
        Err(error) => {
            // A transport error may follow a successful commit. Keep the object in that
            // case: an orphan is recoverable, but deleting committed content is not.
            if matches!(error, StoreError::Conflict(_) | StoreError::NotFound(_)) {
                if let Err(cleanup_error) = objects.delete(object_key).await {
                    // Object keys are generated identifiers, never user credentials or source URLs.
                    tracing::warn!(%object_key, error = %cleanup_error, "failed to remove unreferenced skill archive");
                }
            } else {
                tracing::warn!(%object_key, "skill metadata write failed; preserving archive for reconciliation");
            }
            Err(error.into())
        }
    }
}

fn version_metadata(bundle: &ValidatedBundle) -> Result<SkillVersionMetadata, GatewayError> {
    Ok(SkillVersionMetadata {
        description: bundle.manifest.description.clone(),
        sha256: bundle.sha256.clone(),
        object_key: format!("{}.zip", Uuid::new_v4()),
        archive_bytes: bundle.archive.len() as u64,
        extracted_bytes: bundle.extracted_bytes,
        file_count: u32::try_from(bundle.files.len())
            .map_err(|_| GatewayError::InvalidRequest("too many skill files".into()))?,
        manifest: bundle.manifest.clone(),
        files: bundle.files.clone(),
        instructions: bundle.instructions.clone(),
    })
}

fn namespace_view(record: SkillNamespaceRecord) -> SkillNamespace {
    SkillNamespace {
        user_id: record.user_id,
        handle: record.handle,
    }
}

fn version_summary(record: &SkillVersionRecord) -> SkillVersionSummary {
    SkillVersionSummary {
        version: record.version,
        sha256: record.metadata.sha256.clone(),
        archive_bytes: record.metadata.archive_bytes,
        extracted_bytes: record.metadata.extracted_bytes,
        file_count: record.metadata.file_count,
        created_at: record.created_at,
    }
}

fn invalid_bundle(error: gateway_skills::BundleError) -> GatewayError {
    GatewayError::InvalidRequest(error.to_string())
}

#[cfg(test)]
mod tests;
