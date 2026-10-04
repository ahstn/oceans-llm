use async_trait::async_trait;
use gateway_core::StoreError;
use gateway_core::skills::{
    SkillDetail, SkillListQuery, SkillNamespaceRecord, SkillRecord, SkillRepository,
    SkillUploadResponse, SkillVersionMetadata, SkillVersionRecord, SkillVersionSummary,
};
use time::OffsetDateTime;
use uuid::Uuid;

use super::{
    LibsqlStore,
    support::{to_query_error, to_write_error},
};
use crate::shared::{parse_uuid, serialize_json, unix_to_datetime};

const SKILL_SELECT: &str = "SELECT s.skill_id, s.owner_user_id, n.handle, s.name, s.description, s.default_version, s.latest_version, s.created_at, s.updated_at FROM skills s JOIN skill_namespaces n ON n.user_id = s.owner_user_id";
const VERSION_SELECT: &str = "SELECT skill_id, version, description, sha256, object_key, archive_bytes, extracted_bytes, file_count, manifest_json, files_json, instructions, created_at FROM skill_versions";

impl LibsqlStore {
    // Separate connections prevent a skill write from joining another request's transaction.
    pub(super) async fn skill_connection(&self) -> Result<libsql::Connection, StoreError> {
        let connection = self.database.connect().map_err(to_query_error)?;
        Self::configure_connection(&connection)
            .await
            .map_err(to_query_error)?;
        Ok(connection)
    }
}

fn decode_namespace(row: &libsql::Row) -> Result<SkillNamespaceRecord, StoreError> {
    Ok(SkillNamespaceRecord {
        user_id: parse_uuid(&row.get::<String>(0).map_err(to_query_error)?)?,
        handle: row.get(1).map_err(to_query_error)?,
        created_at: unix_to_datetime(row.get(2).map_err(to_query_error)?)?,
    })
}

fn decode_skill(row: &libsql::Row) -> Result<SkillRecord, StoreError> {
    Ok(SkillRecord {
        id: parse_uuid(&row.get::<String>(0).map_err(to_query_error)?)?,
        owner_user_id: parse_uuid(&row.get::<String>(1).map_err(to_query_error)?)?,
        namespace: row.get(2).map_err(to_query_error)?,
        name: row.get(3).map_err(to_query_error)?,
        description: row.get(4).map_err(to_query_error)?,
        default_version: read_u32(row, 5)?,
        latest_version: read_u32(row, 6)?,
        created_at: unix_to_datetime(row.get(7).map_err(to_query_error)?)?,
        updated_at: unix_to_datetime(row.get(8).map_err(to_query_error)?)?,
    })
}

fn decode_version(row: &libsql::Row) -> Result<SkillVersionRecord, StoreError> {
    let manifest: String = row.get(8).map_err(to_query_error)?;
    let files: String = row.get(9).map_err(to_query_error)?;
    Ok(SkillVersionRecord {
        skill_id: parse_uuid(&row.get::<String>(0).map_err(to_query_error)?)?,
        version: read_u32(row, 1)?,
        metadata: SkillVersionMetadata {
            description: row.get(2).map_err(to_query_error)?,
            sha256: row.get(3).map_err(to_query_error)?,
            object_key: row.get(4).map_err(to_query_error)?,
            archive_bytes: read_u64(row, 5)?,
            extracted_bytes: read_u64(row, 6)?,
            file_count: read_u32(row, 7)?,
            manifest: serde_json::from_str(&manifest)
                .map_err(|error| StoreError::Serialization(error.to_string()))?,
            files: serde_json::from_str(&files)
                .map_err(|error| StoreError::Serialization(error.to_string()))?,
            instructions: row.get(10).map_err(to_query_error)?,
        },
        created_at: unix_to_datetime(row.get(11).map_err(to_query_error)?)?,
    })
}

fn read_u32(row: &libsql::Row, column: i32) -> Result<u32, StoreError> {
    let value: i64 = row.get(column).map_err(to_query_error)?;
    u32::try_from(value).map_err(|error| StoreError::Serialization(error.to_string()))
}

fn read_u64(row: &libsql::Row, column: i32) -> Result<u64, StoreError> {
    let value: i64 = row.get(column).map_err(to_query_error)?;
    u64::try_from(value).map_err(|error| StoreError::Serialization(error.to_string()))
}

fn to_i64(value: u64) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|error| StoreError::Serialization(error.to_string()))
}

async fn insert_version(
    tx: &libsql::Transaction,
    skill_id: Uuid,
    version: u32,
    metadata: &SkillVersionMetadata,
    now: OffsetDateTime,
) -> Result<(), StoreError> {
    tx.execute(
        "INSERT INTO skill_versions (skill_id, version, description, sha256, object_key, archive_bytes, extracted_bytes, file_count, manifest_json, files_json, instructions, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        libsql::params![
            skill_id.to_string(), i64::from(version), metadata.description.clone(),
            metadata.sha256.clone(), metadata.object_key.clone(), to_i64(metadata.archive_bytes)?,
            to_i64(metadata.extracted_bytes)?, i64::from(metadata.file_count),
            serialize_json(&metadata.manifest)?, serialize_json(&metadata.files)?,
            metadata.instructions.clone(), now.unix_timestamp(),
        ],
    ).await.map_err(to_write_error)?;
    Ok(())
}

async fn read_versions(
    connection: &libsql::Connection,
    skill_id: Uuid,
) -> Result<Vec<SkillVersionSummary>, StoreError> {
    let mut rows = connection
        .query(
            "SELECT version, sha256, archive_bytes, extracted_bytes, file_count, created_at FROM skill_versions WHERE skill_id = ?1 ORDER BY version DESC",
            [skill_id.to_string()],
        )
        .await
        .map_err(to_query_error)?;
    let mut versions = Vec::new();
    while let Some(row) = rows.next().await.map_err(to_query_error)? {
        versions.push(SkillVersionSummary {
            version: read_u32(&row, 0)?,
            sha256: row.get(1).map_err(to_query_error)?,
            archive_bytes: read_u64(&row, 2)?,
            extracted_bytes: read_u64(&row, 3)?,
            file_count: read_u32(&row, 4)?,
            created_at: unix_to_datetime(row.get(5).map_err(to_query_error)?)?,
        });
    }
    Ok(versions)
}

async fn upload_response(
    tx: &libsql::Transaction,
    skill_id: Uuid,
    uploaded_version: u32,
) -> Result<SkillUploadResponse, StoreError> {
    let mut rows = tx
        .query(
            &format!("{SKILL_SELECT} WHERE s.skill_id = ?1"),
            [skill_id.to_string()],
        )
        .await
        .map_err(to_query_error)?;
    let row = rows
        .next()
        .await
        .map_err(to_query_error)?
        .ok_or_else(|| StoreError::NotFound("skill not found".to_string()))?;
    let skill = decode_skill(&row)?;
    let versions = read_versions(tx, skill_id).await?;
    Ok(SkillUploadResponse {
        detail: SkillDetail { skill, versions },
        uploaded_version,
    })
}

#[async_trait]
impl SkillRepository for LibsqlStore {
    async fn claim_skill_namespace(
        &self,
        user_id: Uuid,
        handle: &str,
        now: OffsetDateTime,
    ) -> Result<SkillNamespaceRecord, StoreError> {
        let connection = self.skill_connection().await?;
        connection.execute(
            "INSERT INTO skill_namespaces (user_id, handle, created_at) VALUES (?1, ?2, ?3) ON CONFLICT (user_id) DO NOTHING",
            libsql::params![user_id.to_string(), handle, now.unix_timestamp()],
        ).await.map_err(to_write_error)?;
        let record = self
            .get_skill_namespace(user_id)
            .await?
            .ok_or_else(|| StoreError::NotFound("skill namespace not found".to_string()))?;
        if record.handle != handle {
            return Err(StoreError::Conflict(
                "skill namespace cannot be changed".to_string(),
            ));
        }
        Ok(record)
    }

    async fn get_skill_namespace(
        &self,
        user_id: Uuid,
    ) -> Result<Option<SkillNamespaceRecord>, StoreError> {
        let connection = self.skill_connection().await?;
        let mut rows = connection
            .query(
                "SELECT user_id, handle, created_at FROM skill_namespaces WHERE user_id = ?1",
                [user_id.to_string()],
            )
            .await
            .map_err(to_query_error)?;
        rows.next()
            .await
            .map_err(to_query_error)?
            .as_ref()
            .map(decode_namespace)
            .transpose()
    }

    async fn list_skills(&self, query: &SkillListQuery) -> Result<Vec<SkillRecord>, StoreError> {
        let connection = self.skill_connection().await?;
        let search = query.q.as_deref().map(str::trim).filter(|q| !q.is_empty());
        let mut rows = connection
            .query(
                &format!(
                    "{SKILL_SELECT}
                 WHERE (?1 IS NULL OR n.handle = ?1)
                   AND (?2 IS NULL
                        OR instr(lower(s.name), lower(?2)) > 0
                        OR instr(lower(s.description), lower(?2)) > 0
                        OR instr(lower(n.handle), lower(?2)) > 0)
                 ORDER BY s.updated_at DESC, s.skill_id LIMIT ?3 OFFSET ?4"
                ),
                libsql::params![
                    query.namespace.clone(),
                    search,
                    i64::from(query.limit.min(500)),
                    i64::from(query.offset)
                ],
            )
            .await
            .map_err(to_query_error)?;
        let mut skills = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            skills.push(decode_skill(&row)?);
        }
        Ok(skills)
    }

    async fn get_skill(&self, skill_id: Uuid) -> Result<Option<SkillRecord>, StoreError> {
        let connection = self.skill_connection().await?;
        let mut rows = connection
            .query(
                &format!("{SKILL_SELECT} WHERE s.skill_id = ?1"),
                [skill_id.to_string()],
            )
            .await
            .map_err(to_query_error)?;
        rows.next()
            .await
            .map_err(to_query_error)?
            .as_ref()
            .map(decode_skill)
            .transpose()
    }

    async fn get_skill_by_name(
        &self,
        namespace: &str,
        name: &str,
    ) -> Result<Option<SkillRecord>, StoreError> {
        let connection = self.skill_connection().await?;
        let mut rows = connection
            .query(
                &format!("{SKILL_SELECT} WHERE n.handle = ?1 AND s.name = ?2"),
                libsql::params![namespace, name],
            )
            .await
            .map_err(to_query_error)?;
        rows.next()
            .await
            .map_err(to_query_error)?
            .as_ref()
            .map(decode_skill)
            .transpose()
    }

    async fn create_skill(
        &self,
        owner_user_id: Uuid,
        name: &str,
        metadata: &SkillVersionMetadata,
        now: OffsetDateTime,
    ) -> Result<SkillUploadResponse, StoreError> {
        let connection = self.skill_connection().await?;
        let skill_id = Uuid::new_v4();
        let tx = connection
            .transaction_with_behavior(libsql::TransactionBehavior::Immediate)
            .await
            .map_err(to_query_error)?;
        let inserted = tx.execute(
            "INSERT INTO skills (skill_id, owner_user_id, name, description, default_version, latest_version, created_at, updated_at) SELECT ?1, user_id, ?3, ?4, 1, 1, ?5, ?5 FROM skill_namespaces WHERE user_id = ?2",
            libsql::params![skill_id.to_string(), owner_user_id.to_string(), name, metadata.description.clone(), now.unix_timestamp()],
        ).await.map_err(to_write_error)?;
        if inserted == 0 {
            return Err(StoreError::NotFound(
                "skill namespace was not found".to_string(),
            ));
        }
        insert_version(&tx, skill_id, 1, metadata, now).await?;
        let response = upload_response(&tx, skill_id, 1).await?;
        tx.commit().await.map_err(to_write_error)?;
        Ok(response)
    }

    async fn append_skill_version(
        &self,
        owner_user_id: Uuid,
        skill_id: Uuid,
        metadata: &SkillVersionMetadata,
        now: OffsetDateTime,
    ) -> Result<SkillUploadResponse, StoreError> {
        let connection = self.skill_connection().await?;
        let tx = connection
            .transaction_with_behavior(libsql::TransactionBehavior::Immediate)
            .await
            .map_err(to_query_error)?;
        let mut rows = tx.query(
            "UPDATE skills SET latest_version = latest_version + 1, updated_at = ?1 WHERE skill_id = ?2 AND owner_user_id = ?3 AND latest_version < 4294967295 RETURNING latest_version",
            libsql::params![now.unix_timestamp(), skill_id.to_string(), owner_user_id.to_string()],
        ).await.map_err(to_write_error)?;
        let Some(row) = rows.next().await.map_err(to_query_error)? else {
            drop(rows);
            let mut owned = tx
                .query(
                    "SELECT skill_id FROM skills WHERE skill_id = ?1 AND owner_user_id = ?2",
                    libsql::params![skill_id.to_string(), owner_user_id.to_string()],
                )
                .await
                .map_err(to_query_error)?;
            return Err(if owned.next().await.map_err(to_query_error)?.is_some() {
                StoreError::Conflict("skill version limit reached".to_string())
            } else {
                StoreError::NotFound("owned skill was not found".to_string())
            });
        };
        let version = read_u32(&row, 0)?;
        drop(row);
        // Step RETURNING through SQLITE_DONE before committing its transaction.
        rows.next().await.map_err(to_query_error)?;
        drop(rows);
        insert_version(&tx, skill_id, version, metadata, now).await?;
        let response = upload_response(&tx, skill_id, version).await?;
        tx.commit().await.map_err(to_write_error)?;
        Ok(response)
    }

    async fn list_skill_versions(
        &self,
        skill_id: Uuid,
    ) -> Result<Vec<SkillVersionSummary>, StoreError> {
        let connection = self.skill_connection().await?;
        read_versions(&connection, skill_id).await
    }

    async fn get_skill_version(
        &self,
        skill_id: Uuid,
        version: u32,
    ) -> Result<Option<SkillVersionRecord>, StoreError> {
        let connection = self.skill_connection().await?;
        let mut rows = connection
            .query(
                &format!("{VERSION_SELECT} WHERE skill_id = ?1 AND version = ?2"),
                libsql::params![skill_id.to_string(), i64::from(version)],
            )
            .await
            .map_err(to_query_error)?;
        rows.next()
            .await
            .map_err(to_query_error)?
            .as_ref()
            .map(decode_version)
            .transpose()
    }

    async fn set_skill_default_version(
        &self,
        owner_user_id: Uuid,
        skill_id: Uuid,
        version: u32,
        now: OffsetDateTime,
    ) -> Result<SkillRecord, StoreError> {
        let connection = self.skill_connection().await?;
        let changed = connection.execute(
            "UPDATE skills SET default_version = ?1, description = (SELECT description FROM skill_versions WHERE skill_id = ?2 AND version = ?1), updated_at = ?3 WHERE skill_id = ?2 AND owner_user_id = ?4 AND EXISTS (SELECT 1 FROM skill_versions WHERE skill_id = ?2 AND version = ?1)",
            libsql::params![i64::from(version), skill_id.to_string(), now.unix_timestamp(), owner_user_id.to_string()],
        ).await.map_err(to_write_error)?;
        if changed == 0 {
            return Err(StoreError::NotFound(
                "owned skill version not found".to_string(),
            ));
        }
        self.get_skill(skill_id)
            .await?
            .ok_or_else(|| StoreError::NotFound("skill not found".to_string()))
    }
}
