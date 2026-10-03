use super::*;
use crate::shared::{parse_uuid, serialize_json, unix_to_datetime};
use gateway_core::skills::SkillVersionSummary;
use gateway_core::{
    SkillListQuery, SkillNamespaceRecord, SkillRecord, SkillRepository, SkillVersionMetadata,
    SkillVersionRecord,
};

const SKILL_COLUMNS: &str = "s.skill_id, n.handle, s.owner_user_id, s.name, s.description, s.default_version, s.latest_version, s.created_at, s.updated_at";
const VERSION_COLUMNS: &str = "skill_id, version, description, sha256, object_key, archive_bytes, extracted_bytes, file_count, manifest_json, files_json, instructions, created_at";

fn decode_namespace(row: &PgRow) -> Result<SkillNamespaceRecord, StoreError> {
    Ok(SkillNamespaceRecord {
        user_id: parse_uuid(
            &row.try_get::<String, _>("user_id")
                .map_err(to_query_error)?,
        )?,
        handle: row.try_get("handle").map_err(to_query_error)?,
        created_at: unix_to_datetime(row.try_get("created_at").map_err(to_query_error)?)?,
    })
}

fn read_u32(row: &PgRow, column: &str) -> Result<u32, StoreError> {
    let value: i64 = row.try_get(column).map_err(to_query_error)?;
    u32::try_from(value)
        .map_err(|_| StoreError::Serialization(format!("invalid skill {column}: {value}")))
}

fn read_u64(row: &PgRow, column: &str) -> Result<u64, StoreError> {
    let value: i64 = row.try_get(column).map_err(to_query_error)?;
    u64::try_from(value)
        .map_err(|_| StoreError::Serialization(format!("invalid skill {column}: {value}")))
}

fn decode_skill(row: &PgRow) -> Result<SkillRecord, StoreError> {
    Ok(SkillRecord {
        id: parse_uuid(
            &row.try_get::<String, _>("skill_id")
                .map_err(to_query_error)?,
        )?,
        namespace: row.try_get("handle").map_err(to_query_error)?,
        owner_user_id: parse_uuid(
            &row.try_get::<String, _>("owner_user_id")
                .map_err(to_query_error)?,
        )?,
        name: row.try_get("name").map_err(to_query_error)?,
        description: row.try_get("description").map_err(to_query_error)?,
        default_version: read_u32(row, "default_version")?,
        latest_version: read_u32(row, "latest_version")?,
        created_at: unix_to_datetime(row.try_get("created_at").map_err(to_query_error)?)?,
        updated_at: unix_to_datetime(row.try_get("updated_at").map_err(to_query_error)?)?,
    })
}

fn decode_version(row: &PgRow) -> Result<SkillVersionRecord, StoreError> {
    let manifest: String = row.try_get("manifest_json").map_err(to_query_error)?;
    let files: String = row.try_get("files_json").map_err(to_query_error)?;
    Ok(SkillVersionRecord {
        skill_id: parse_uuid(
            &row.try_get::<String, _>("skill_id")
                .map_err(to_query_error)?,
        )?,
        version: read_u32(row, "version")?,
        metadata: SkillVersionMetadata {
            description: row.try_get("description").map_err(to_query_error)?,
            sha256: row.try_get("sha256").map_err(to_query_error)?,
            object_key: row.try_get("object_key").map_err(to_query_error)?,
            archive_bytes: read_u64(row, "archive_bytes")?,
            extracted_bytes: read_u64(row, "extracted_bytes")?,
            file_count: read_u32(row, "file_count")?,
            manifest: serde_json::from_str(&manifest)
                .map_err(|error| StoreError::Serialization(error.to_string()))?,
            files: serde_json::from_str(&files)
                .map_err(|error| StoreError::Serialization(error.to_string()))?,
            instructions: row.try_get("instructions").map_err(to_query_error)?,
        },
        created_at: unix_to_datetime(row.try_get("created_at").map_err(to_query_error)?)?,
    })
}

fn decode_version_summary(row: &PgRow) -> Result<SkillVersionSummary, StoreError> {
    Ok(SkillVersionSummary {
        version: read_u32(row, "version")?,
        sha256: row.try_get("sha256").map_err(to_query_error)?,
        archive_bytes: read_u64(row, "archive_bytes")?,
        extracted_bytes: read_u64(row, "extracted_bytes")?,
        file_count: read_u32(row, "file_count")?,
        created_at: unix_to_datetime(row.try_get("created_at").map_err(to_query_error)?)?,
    })
}

fn skill_write_error(error: sqlx::Error) -> StoreError {
    if error
        .as_database_error()
        .and_then(|error| error.code())
        .as_deref()
        == Some("23505")
    {
        StoreError::Conflict("skill identity, version, or object key already exists".to_string())
    } else {
        to_query_error(error)
    }
}

async fn insert_version(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    skill_id: Uuid,
    version: u32,
    metadata: &SkillVersionMetadata,
    now: OffsetDateTime,
) -> Result<SkillVersionRecord, StoreError> {
    let archive_bytes = i64::try_from(metadata.archive_bytes)
        .map_err(|_| StoreError::Serialization("skill archive size exceeds i64".to_string()))?;
    let extracted_bytes = i64::try_from(metadata.extracted_bytes)
        .map_err(|_| StoreError::Serialization("skill extracted size exceeds i64".to_string()))?;
    let sql = format!(
        "INSERT INTO skill_versions ({VERSION_COLUMNS}) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) RETURNING {VERSION_COLUMNS}"
    );
    let row = sqlx::query(&sql)
        .bind(skill_id.to_string())
        .bind(i64::from(version))
        .bind(&metadata.description)
        .bind(&metadata.sha256)
        .bind(&metadata.object_key)
        .bind(archive_bytes)
        .bind(extracted_bytes)
        .bind(i64::from(metadata.file_count))
        .bind(serialize_json(&metadata.manifest)?)
        .bind(serialize_json(&metadata.files)?)
        .bind(&metadata.instructions)
        .bind(now.unix_timestamp())
        .fetch_one(&mut **tx)
        .await
        .map_err(skill_write_error)?;
    decode_version(&row)
}

#[async_trait]
impl SkillRepository for PostgresStore {
    async fn claim_skill_namespace(
        &self,
        user_id: Uuid,
        handle: &str,
        now: OffsetDateTime,
    ) -> Result<SkillNamespaceRecord, StoreError> {
        sqlx::query(
            "INSERT INTO skill_namespaces (user_id, handle, created_at) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
        )
        .bind(user_id.to_string())
        .bind(handle)
        .bind(now.unix_timestamp())
        .execute(&self.pool)
        .await
        .map_err(to_query_error)?;
        match self.get_skill_namespace(user_id).await? {
            Some(namespace) if namespace.handle == handle => Ok(namespace),
            _ => Err(StoreError::Conflict(
                "skill namespace is already claimed or cannot be changed".to_string(),
            )),
        }
    }

    async fn get_skill_namespace(
        &self,
        user_id: Uuid,
    ) -> Result<Option<SkillNamespaceRecord>, StoreError> {
        sqlx::query("SELECT user_id, handle, created_at FROM skill_namespaces WHERE user_id = $1")
            .bind(user_id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(to_query_error)?
            .as_ref()
            .map(decode_namespace)
            .transpose()
    }

    async fn list_skills(&self, query: &SkillListQuery) -> Result<Vec<SkillRecord>, StoreError> {
        let sql = format!(
            "SELECT {SKILL_COLUMNS} FROM skills s JOIN skill_namespaces n ON n.user_id = s.owner_user_id WHERE ($1::text IS NULL OR n.handle = $1) ORDER BY s.updated_at DESC, s.skill_id LIMIT $2 OFFSET $3"
        );
        let rows = sqlx::query(&sql)
            .bind(query.namespace.as_deref())
            .bind(i64::from(query.limit.min(500)))
            .bind(i64::from(query.offset))
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?;
        rows.iter().map(decode_skill).collect()
    }

    async fn get_skill(&self, skill_id: Uuid) -> Result<Option<SkillRecord>, StoreError> {
        let sql = format!(
            "SELECT {SKILL_COLUMNS} FROM skills s JOIN skill_namespaces n ON n.user_id = s.owner_user_id WHERE s.skill_id = $1"
        );
        sqlx::query(&sql)
            .bind(skill_id.to_string())
            .fetch_optional(&self.pool)
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
        let sql = format!(
            "SELECT {SKILL_COLUMNS} FROM skills s JOIN skill_namespaces n ON n.user_id = s.owner_user_id WHERE n.handle = $1 AND s.name = $2"
        );
        sqlx::query(&sql)
            .bind(namespace)
            .bind(name)
            .fetch_optional(&self.pool)
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
    ) -> Result<SkillVersionRecord, StoreError> {
        let skill_id = Uuid::new_v4();
        let mut tx = self.pool.begin().await.map_err(to_query_error)?;
        let inserted = sqlx::query(
            "INSERT INTO skills (skill_id, owner_user_id, name, description, default_version, latest_version, created_at, updated_at) SELECT $1, user_id, $3, $4, 1, 1, $5, $5 FROM skill_namespaces WHERE user_id = $2",
        )
        .bind(skill_id.to_string())
        .bind(owner_user_id.to_string())
        .bind(name)
        .bind(&metadata.description)
        .bind(now.unix_timestamp())
        .execute(&mut *tx)
        .await
        .map_err(skill_write_error)?;
        if inserted.rows_affected() == 0 {
            return Err(StoreError::NotFound(
                "skill namespace was not found".to_string(),
            ));
        }
        let version = insert_version(&mut tx, skill_id, 1, metadata, now).await?;
        tx.commit().await.map_err(to_query_error)?;
        Ok(version)
    }

    async fn append_skill_version(
        &self,
        owner_user_id: Uuid,
        skill_id: Uuid,
        metadata: &SkillVersionMetadata,
        now: OffsetDateTime,
    ) -> Result<SkillVersionRecord, StoreError> {
        let mut tx = self.pool.begin().await.map_err(to_query_error)?;
        let row = sqlx::query(
            "UPDATE skills SET latest_version = latest_version + 1, updated_at = $3 WHERE skill_id = $1 AND owner_user_id = $2 AND latest_version < 4294967295 RETURNING latest_version",
        )
        .bind(skill_id.to_string())
        .bind(owner_user_id.to_string())
        .bind(now.unix_timestamp())
        .fetch_optional(&mut *tx)
        .await
        .map_err(to_query_error)?;
        let Some(row) = row else {
            let owned = sqlx::query(
                "SELECT skill_id FROM skills WHERE skill_id = $1 AND owner_user_id = $2",
            )
            .bind(skill_id.to_string())
            .bind(owner_user_id.to_string())
            .fetch_optional(&mut *tx)
            .await
            .map_err(to_query_error)?;
            return Err(if owned.is_some() {
                StoreError::Conflict("skill version limit reached".to_string())
            } else {
                StoreError::NotFound("owned skill was not found".to_string())
            });
        };
        let version = read_u32(&row, "latest_version")?;
        let version = insert_version(&mut tx, skill_id, version, metadata, now).await?;
        tx.commit().await.map_err(to_query_error)?;
        Ok(version)
    }

    async fn list_skill_versions(
        &self,
        skill_id: Uuid,
    ) -> Result<Vec<SkillVersionSummary>, StoreError> {
        let rows = sqlx::query(
            "SELECT version, sha256, archive_bytes, extracted_bytes, file_count, created_at FROM skill_versions WHERE skill_id = $1 ORDER BY version DESC",
        )
        .bind(skill_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;
        rows.iter().map(decode_version_summary).collect()
    }

    async fn get_skill_version(
        &self,
        skill_id: Uuid,
        version: u32,
    ) -> Result<Option<SkillVersionRecord>, StoreError> {
        let sql = format!(
            "SELECT {VERSION_COLUMNS} FROM skill_versions WHERE skill_id = $1 AND version = $2"
        );
        sqlx::query(&sql)
            .bind(skill_id.to_string())
            .bind(i64::from(version))
            .fetch_optional(&self.pool)
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
        let sql = format!(
            "UPDATE skills s SET default_version = v.version, description = v.description, updated_at = $4 FROM skill_versions v, skill_namespaces n WHERE s.skill_id = $1 AND s.owner_user_id = $2 AND v.skill_id = s.skill_id AND v.version = $3 AND n.user_id = s.owner_user_id RETURNING {SKILL_COLUMNS}"
        );
        let row = sqlx::query(&sql)
            .bind(skill_id.to_string())
            .bind(owner_user_id.to_string())
            .bind(i64::from(version))
            .bind(now.unix_timestamp())
            .fetch_optional(&self.pool)
            .await
            .map_err(to_query_error)?
            .ok_or_else(|| StoreError::NotFound("owned skill version was not found".to_string()))?;
        decode_skill(&row)
    }
}
