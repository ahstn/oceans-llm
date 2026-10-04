//! Persistent skill identities and storage boundaries.

use async_trait::async_trait;
pub use gateway_skills::{
    SkillDetail, SkillFile, SkillManifest, SkillSummary as SkillRecord, SkillUploadResponse,
    SkillVersionSummary,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::StoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillNamespaceRecord {
    pub user_id: Uuid,
    pub handle: String,
    pub created_at: OffsetDateTime,
}

/// Content that has passed bundle validation before it reaches persistence.
#[derive(Debug, Clone, PartialEq)]
pub struct SkillVersionMetadata {
    pub description: String,
    pub sha256: String,
    pub object_key: String,
    pub archive_bytes: u64,
    pub extracted_bytes: u64,
    pub file_count: u32,
    pub manifest: SkillManifest,
    pub files: Vec<SkillFile>,
    pub instructions: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SkillVersionRecord {
    pub skill_id: Uuid,
    pub version: u32,
    pub metadata: SkillVersionMetadata,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct SkillListQuery {
    pub namespace: Option<String>,
    /// Literal substring of name, description, or owner namespace, ignoring ASCII letter case.
    /// Other characters match exactly. Blank input is ignored.
    pub q: Option<String>,
    pub limit: u32,
    pub offset: u32,
}

impl Default for SkillListQuery {
    fn default() -> Self {
        Self {
            namespace: None,
            q: None,
            limit: 100,
            offset: 0,
        }
    }
}

#[async_trait]
pub trait SkillRepository: Send + Sync {
    /// Claim once; repeating the same owner's handle is idempotent.
    async fn claim_skill_namespace(
        &self,
        user_id: Uuid,
        handle: &str,
        now: OffsetDateTime,
    ) -> Result<SkillNamespaceRecord, StoreError>;
    async fn get_skill_namespace(
        &self,
        user_id: Uuid,
    ) -> Result<Option<SkillNamespaceRecord>, StoreError>;
    async fn list_skills(&self, query: &SkillListQuery) -> Result<Vec<SkillRecord>, StoreError>;
    async fn get_skill(&self, skill_id: Uuid) -> Result<Option<SkillRecord>, StoreError>;
    async fn get_skill_by_name(
        &self,
        namespace: &str,
        name: &str,
    ) -> Result<Option<SkillRecord>, StoreError>;
    /// Create an owned skill and its first/default version in one transaction.
    /// Build the response within that transaction before committing it.
    async fn create_skill(
        &self,
        owner_user_id: Uuid,
        name: &str,
        metadata: &SkillVersionMetadata,
        now: OffsetDateTime,
    ) -> Result<SkillUploadResponse, StoreError>;
    /// Allocate a version atomically; leave the default version unchanged.
    /// Build the response within that transaction before committing it.
    async fn append_skill_version(
        &self,
        owner_user_id: Uuid,
        skill_id: Uuid,
        metadata: &SkillVersionMetadata,
        now: OffsetDateTime,
    ) -> Result<SkillUploadResponse, StoreError>;
    async fn list_skill_versions(
        &self,
        skill_id: Uuid,
    ) -> Result<Vec<SkillVersionSummary>, StoreError>;
    async fn get_skill_version(
        &self,
        skill_id: Uuid,
        version: u32,
    ) -> Result<Option<SkillVersionRecord>, StoreError>;
    /// Update only an owned skill and only to one of its existing versions.
    async fn set_skill_default_version(
        &self,
        owner_user_id: Uuid,
        skill_id: Uuid,
        version: u32,
        now: OffsetDateTime,
    ) -> Result<SkillRecord, StoreError>;
}

#[async_trait]
pub trait SkillObjectStore: Send + Sync {
    async fn put(&self, key: &str, bytes: &[u8]) -> Result<(), StoreError>;
    /// Read within the bound recorded for this immutable archive, independently
    /// of the current upload policy.
    async fn get(&self, key: &str, max_bytes: u64) -> Result<Vec<u8>, StoreError>;
    async fn delete(&self, key: &str) -> Result<(), StoreError>;
}
