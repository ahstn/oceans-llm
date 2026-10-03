use async_trait::async_trait;
use gateway_core::StoreError;
use gateway_core::skills::{
    SkillListQuery, SkillNamespaceRecord, SkillRecord, SkillRepository, SkillVersionMetadata,
    SkillVersionRecord, SkillVersionSummary,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::AnyStore;

macro_rules! dispatch_store {
    ($self:expr, $method:ident ( $($arg:expr),* )) => {
        match $self {
            AnyStore::Libsql(store) => store.$method($($arg),*).await,
            AnyStore::Postgres(store) => store.$method($($arg),*).await,
        }
    };
}

#[async_trait]
impl SkillRepository for AnyStore {
    async fn claim_skill_namespace(
        &self,
        user_id: Uuid,
        handle: &str,
        now: OffsetDateTime,
    ) -> Result<SkillNamespaceRecord, StoreError> {
        dispatch_store!(self, claim_skill_namespace(user_id, handle, now))
    }
    async fn get_skill_namespace(
        &self,
        user_id: Uuid,
    ) -> Result<Option<SkillNamespaceRecord>, StoreError> {
        dispatch_store!(self, get_skill_namespace(user_id))
    }
    async fn list_skills(&self, query: &SkillListQuery) -> Result<Vec<SkillRecord>, StoreError> {
        dispatch_store!(self, list_skills(query))
    }
    async fn get_skill(&self, skill_id: Uuid) -> Result<Option<SkillRecord>, StoreError> {
        dispatch_store!(self, get_skill(skill_id))
    }
    async fn get_skill_by_name(
        &self,
        namespace: &str,
        name: &str,
    ) -> Result<Option<SkillRecord>, StoreError> {
        dispatch_store!(self, get_skill_by_name(namespace, name))
    }
    async fn create_skill(
        &self,
        owner_user_id: Uuid,
        name: &str,
        metadata: &SkillVersionMetadata,
        now: OffsetDateTime,
    ) -> Result<SkillVersionRecord, StoreError> {
        dispatch_store!(self, create_skill(owner_user_id, name, metadata, now))
    }
    async fn append_skill_version(
        &self,
        owner_user_id: Uuid,
        skill_id: Uuid,
        metadata: &SkillVersionMetadata,
        now: OffsetDateTime,
    ) -> Result<SkillVersionRecord, StoreError> {
        dispatch_store!(
            self,
            append_skill_version(owner_user_id, skill_id, metadata, now)
        )
    }
    async fn list_skill_versions(
        &self,
        skill_id: Uuid,
    ) -> Result<Vec<SkillVersionSummary>, StoreError> {
        dispatch_store!(self, list_skill_versions(skill_id))
    }
    async fn get_skill_version(
        &self,
        skill_id: Uuid,
        version: u32,
    ) -> Result<Option<SkillVersionRecord>, StoreError> {
        dispatch_store!(self, get_skill_version(skill_id, version))
    }
    async fn set_skill_default_version(
        &self,
        owner_user_id: Uuid,
        skill_id: Uuid,
        version: u32,
        now: OffsetDateTime,
    ) -> Result<SkillRecord, StoreError> {
        dispatch_store!(
            self,
            set_skill_default_version(owner_user_id, skill_id, version, now)
        )
    }
}
