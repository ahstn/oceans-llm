//! Session and API-key access to the shared skill catalog.

mod auth;
#[cfg(test)]
mod tests;

use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use gateway_core::{GatewayError, SkillListQuery, StoreError};
use gateway_skills::{
    BundleLimits, ClaimNamespaceRequest, SetDefaultVersionRequest, SkillDetail, SkillFileContent,
    SkillNamespace, SkillSummary, SkillUploadResponse, SkillVersionDetail, SkillVersionSummary,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::http::{
    error::AppError,
    state::{AppSkillService, AppState},
};
use auth::SkillCaller;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/skills/limits", get(get_skill_limits))
        .route(
            "/api/v1/skills/namespace",
            get(get_skill_namespace).post(claim_skill_namespace),
        )
        .route("/api/v1/skills", get(list_skills).post(create_skill))
        .route(
            "/api/v1/skills/by-name/{namespace}/{name}",
            get(get_skill_by_name),
        )
        .route("/api/v1/skills/{skill_id}", get(get_skill))
        .route(
            "/api/v1/skills/{skill_id}/versions",
            get(list_skill_versions).post(append_skill_version),
        )
        .route(
            "/api/v1/skills/{skill_id}/versions/{version}",
            get(get_skill_version),
        )
        .route(
            "/api/v1/skills/{skill_id}/versions/{version}/archive",
            get(download_skill_archive),
        )
        .route(
            "/api/v1/skills/{skill_id}/versions/{version}/files",
            get(get_skill_file),
        )
        .route(
            "/api/v1/skills/{skill_id}/default-version",
            axum::routing::put(set_skill_default_version),
        )
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct ListSkillsQuery {
    pub namespace: Option<String>,
    /// Literal search in name, description, and owner namespace, ignoring ASCII letter case.
    /// Other characters match exactly. Blank input is ignored.
    pub q: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

fn default_limit() -> u32 {
    100
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct SkillFileQuery {
    pub path: String,
}

#[utoipa::path(get, path = "/api/v1/skills/limits", responses((status = 200, body = BundleLimits)), security(("session_cookie" = []), ("gateway_api_key" = [])))]
pub async fn get_skill_limits(
    State(state): State<AppState>,
    _caller: SkillCaller,
) -> Result<Json<BundleLimits>, AppError> {
    Ok(Json(*service(&state)?.limits()))
}

#[utoipa::path(get, path = "/api/v1/skills/namespace", responses((status = 200, body = Option<SkillNamespace>)), security(("session_cookie" = []), ("gateway_api_key" = [])))]
pub async fn get_skill_namespace(
    State(state): State<AppState>,
    caller: SkillCaller,
) -> Result<Json<Option<SkillNamespace>>, AppError> {
    let service = service(&state)?;
    let namespace = match caller.user_id() {
        Some(user_id) => service.namespace(user_id).await?,
        None => None,
    };
    Ok(Json(namespace))
}

#[utoipa::path(post, path = "/api/v1/skills/namespace", request_body = ClaimNamespaceRequest, responses((status = 201, body = SkillNamespace)), security(("session_cookie" = []), ("gateway_api_key" = [])))]
pub async fn claim_skill_namespace(
    State(state): State<AppState>,
    caller: SkillCaller,
    Json(input): Json<ClaimNamespaceRequest>,
) -> Result<(StatusCode, Json<SkillNamespace>), AppError> {
    let namespace = service(&state)?
        .claim_namespace(caller.require_user()?, &input.handle)
        .await?;
    Ok((StatusCode::CREATED, Json(namespace)))
}

#[utoipa::path(get, path = "/api/v1/skills", params(ListSkillsQuery), responses((status = 200, body = Vec<SkillSummary>)), security(("session_cookie" = []), ("gateway_api_key" = [])))]
pub async fn list_skills(
    State(state): State<AppState>,
    _caller: SkillCaller,
    Query(query): Query<ListSkillsQuery>,
) -> Result<Json<Vec<SkillSummary>>, AppError> {
    let query = SkillListQuery {
        namespace: query.namespace,
        q: query.q,
        limit: query.limit,
        offset: query.offset,
    };
    Ok(Json(service(&state)?.list(&query).await?))
}

#[utoipa::path(post, path = "/api/v1/skills", request_body(content = crate::http::admin_contract::SkillArchiveBody, content_type = "application/zip"), responses((status = 201, body = SkillUploadResponse)), security(("session_cookie" = []), ("gateway_api_key" = [])))]
pub async fn create_skill(
    State(state): State<AppState>,
    caller: SkillCaller,
    request: Request,
) -> Result<(StatusCode, Json<SkillUploadResponse>), AppError> {
    let user_id = caller.require_user()?;
    let service = service(&state)?;
    let bytes = upload_body(service, request).await?;
    Ok((
        StatusCode::CREATED,
        Json(service.create(user_id, bytes).await?),
    ))
}

#[utoipa::path(get, path = "/api/v1/skills/by-name/{namespace}/{name}", params(("namespace" = String, Path), ("name" = String, Path)), responses((status = 200, body = SkillDetail)), security(("session_cookie" = []), ("gateway_api_key" = [])))]
pub async fn get_skill_by_name(
    State(state): State<AppState>,
    _caller: SkillCaller,
    Path((namespace, name)): Path<(String, String)>,
) -> Result<Json<SkillDetail>, AppError> {
    Ok(Json(service(&state)?.by_name(&namespace, &name).await?))
}

#[utoipa::path(get, path = "/api/v1/skills/{skill_id}", params(("skill_id" = Uuid, Path)), responses((status = 200, body = SkillDetail)), security(("session_cookie" = []), ("gateway_api_key" = [])))]
pub async fn get_skill(
    State(state): State<AppState>,
    _caller: SkillCaller,
    Path(id): Path<Uuid>,
) -> Result<Json<SkillDetail>, AppError> {
    Ok(Json(service(&state)?.detail(id).await?))
}

#[utoipa::path(get, path = "/api/v1/skills/{skill_id}/versions", params(("skill_id" = Uuid, Path)), responses((status = 200, body = Vec<SkillVersionSummary>)), security(("session_cookie" = []), ("gateway_api_key" = [])))]
pub async fn list_skill_versions(
    State(state): State<AppState>,
    _caller: SkillCaller,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<SkillVersionSummary>>, AppError> {
    Ok(Json(service(&state)?.versions(id).await?))
}

#[utoipa::path(post, path = "/api/v1/skills/{skill_id}/versions", params(("skill_id" = Uuid, Path)), request_body(content = crate::http::admin_contract::SkillArchiveBody, content_type = "application/zip"), responses((status = 201, body = SkillUploadResponse)), security(("session_cookie" = []), ("gateway_api_key" = [])))]
pub async fn append_skill_version(
    State(state): State<AppState>,
    caller: SkillCaller,
    Path(id): Path<Uuid>,
    request: Request,
) -> Result<(StatusCode, Json<SkillUploadResponse>), AppError> {
    let user_id = caller.require_user()?;
    let service = service(&state)?;
    let bytes = upload_body(service, request).await?;
    Ok((
        StatusCode::CREATED,
        Json(service.append(user_id, id, bytes).await?),
    ))
}

#[utoipa::path(get, path = "/api/v1/skills/{skill_id}/versions/{version}", params(("skill_id" = Uuid, Path), ("version" = u32, Path)), responses((status = 200, body = SkillVersionDetail)), security(("session_cookie" = []), ("gateway_api_key" = [])))]
pub async fn get_skill_version(
    State(state): State<AppState>,
    _caller: SkillCaller,
    Path((id, version)): Path<(Uuid, u32)>,
) -> Result<Json<SkillVersionDetail>, AppError> {
    Ok(Json(service(&state)?.version(id, version).await?))
}

#[utoipa::path(get, path = "/api/v1/skills/{skill_id}/versions/{version}/archive", params(("skill_id" = Uuid, Path), ("version" = u32, Path)), responses((status = 200, body = crate::http::admin_contract::SkillArchiveBody, content_type = "application/zip")), security(("session_cookie" = []), ("gateway_api_key" = [])))]
pub async fn download_skill_archive(
    State(state): State<AppState>,
    _caller: SkillCaller,
    Path((id, version)): Path<(Uuid, u32)>,
) -> Result<Response, AppError> {
    let (bytes, digest) = service(&state)?.archive(id, version).await?;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=skill.zip"),
    );
    headers.insert(
        "x-skill-sha256",
        HeaderValue::from_str(&digest)
            .map_err(|_| GatewayError::Internal("invalid skill digest".into()))?,
    );
    Ok((headers, Body::from(bytes)).into_response())
}

#[utoipa::path(get, path = "/api/v1/skills/{skill_id}/versions/{version}/files", params(("skill_id" = Uuid, Path), ("version" = u32, Path), SkillFileQuery), responses((status = 200, body = SkillFileContent)), security(("session_cookie" = []), ("gateway_api_key" = [])))]
pub async fn get_skill_file(
    State(state): State<AppState>,
    _caller: SkillCaller,
    Path((id, version)): Path<(Uuid, u32)>,
    Query(query): Query<SkillFileQuery>,
) -> Result<Json<SkillFileContent>, AppError> {
    Ok(Json(service(&state)?.file(id, version, &query.path).await?))
}

#[utoipa::path(put, path = "/api/v1/skills/{skill_id}/default-version", params(("skill_id" = Uuid, Path)), request_body = SetDefaultVersionRequest, responses((status = 200, body = SkillDetail)), security(("session_cookie" = []), ("gateway_api_key" = [])))]
pub async fn set_skill_default_version(
    State(state): State<AppState>,
    caller: SkillCaller,
    Path(id): Path<Uuid>,
    Json(input): Json<SetDefaultVersionRequest>,
) -> Result<Json<SkillDetail>, AppError> {
    Ok(Json(
        service(&state)?
            .set_default(caller.require_user()?, id, input.version)
            .await?,
    ))
}

fn service(state: &AppState) -> Result<&AppSkillService, AppError> {
    state
        .skills
        .as_deref()
        .ok_or_else(|| StoreError::Unavailable("skills are not configured".into()).into())
}

async fn upload_body(service: &AppSkillService, request: Request) -> Result<Vec<u8>, AppError> {
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());
    if !content_type.is_some_and(|value| {
        value
            .split(';')
            .next()
            .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/zip"))
    }) {
        return Err(
            GatewayError::InvalidRequest("skill uploads require application/zip".into()).into(),
        );
    }
    let limit_bytes = usize::try_from(service.limits().max_archive_bytes).unwrap_or(usize::MAX);
    to_bytes(request.into_body(), limit_bytes)
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|error| {
            use std::error::Error;
            if error
                .source()
                .is_some_and(|source| source.is::<http_body_util::LengthLimitError>())
            {
                GatewayError::PayloadTooLarge { limit_bytes }.into()
            } else {
                GatewayError::InvalidRequest("could not read skill upload".into()).into()
            }
        })
}
