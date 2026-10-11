use axum::{
    extract::FromRequestParts,
    http::{HeaderMap, Method, header, request::Parts},
};
use gateway_core::{AuthError, IdentityRepository, UserStatus, extract_bearer_token};
use uuid::Uuid;

use crate::http::{error::AppError, identity::resolve_session_user, state::AppState};

/// Resolves the authenticated owner before any upload body is buffered.
/// A service account is a reader, and never borrows a session's write identity.
pub struct SkillCaller {
    user_id: Option<Uuid>,
}

impl SkillCaller {
    pub fn user_id(&self) -> Option<Uuid> {
        self.user_id
    }

    pub fn require_user(&self) -> Result<Uuid, AppError> {
        self.user_id
            .ok_or_else(|| AuthError::InsufficientPrivileges.into())
    }
}

impl FromRequestParts<AppState> for SkillCaller {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if let Some(token) = api_token(&parts.headers)? {
            let key = state.service.authenticate_bearer_token(token).await?;
            let user_id = key.owner_user_id;
            if let Some(user_id) = user_id {
                let user = state
                    .store
                    .get_user_by_id(user_id)
                    .await?
                    .ok_or(AuthError::InvalidCredentials)?;
                if user.status != UserStatus::Active {
                    return Err(AuthError::InsufficientPrivileges.into());
                }
            }
            return Ok(Self { user_id });
        }
        let user = resolve_session_user(state, &parts.headers)
            .await?
            .ok_or(AuthError::SessionRequired)?;
        if user.status != UserStatus::Active {
            return Err(AuthError::InsufficientPrivileges.into());
        }
        if !matches!(parts.method, Method::GET | Method::HEAD | Method::OPTIONS) {
            require_same_origin(&parts.headers)?;
        }
        Ok(Self {
            user_id: Some(user.user_id),
        })
    }
}

fn api_token(headers: &HeaderMap) -> Result<Option<&str>, AppError> {
    let authorization = header_text(headers, header::AUTHORIZATION.as_str())?;
    let explicit = header_text(headers, "x-oceans-api-key")?;
    let bearer = authorization.map(extract_bearer_token).transpose()?;
    match (bearer, explicit) {
        (Some(left), Some(right)) if left != right => {
            Err(AuthError::ConflictingApiKeyHeaders.into())
        }
        (Some(token), _) | (None, Some(token)) => Ok(Some(token)),
        (None, None) => Ok(None),
    }
}

fn header_text<'a>(headers: &'a HeaderMap, name: &str) -> Result<Option<&'a str>, AppError> {
    headers
        .get(name)
        .map(|value| {
            value
                .to_str()
                .map_err(|_| AuthError::InvalidAuthorizationHeader.into())
        })
        .transpose()
}

fn require_same_origin(headers: &HeaderMap) -> Result<(), AppError> {
    if headers
        .get("sec-fetch-site")
        .is_some_and(|value| value == "cross-site" || value == "same-site")
    {
        return Err(AuthError::InsufficientPrivileges.into());
    }
    if let Some(origin) = headers.get(header::ORIGIN) {
        let origin = origin
            .to_str()
            .ok()
            .and_then(|value| url::Url::parse(value).ok())
            .ok_or(AuthError::InsufficientPrivileges)?;
        let host = headers
            .get(header::HOST)
            .and_then(|value| value.to_str().ok())
            .ok_or(AuthError::InsufficientPrivileges)?;
        let expected = url::Url::parse(&format!("{}://{host}", origin.scheme()))
            .map_err(|_| AuthError::InsufficientPrivileges)?;
        if !matches!(origin.scheme(), "http" | "https") || origin.origin() != expected.origin() {
            return Err(AuthError::InsufficientPrivileges.into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_conflicting_key_headers_and_cross_origin_mutations() {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, "Bearer first".parse().unwrap());
        headers.insert("x-oceans-api-key", "second".parse().unwrap());
        assert!(api_token(&headers).is_err());
        headers.clear();
        headers.insert(header::HOST, "oceans.example".parse().unwrap());
        headers.insert(header::ORIGIN, "https://other.example".parse().unwrap());
        assert!(require_same_origin(&headers).is_err());
        headers.insert(header::ORIGIN, "https://oceans.example".parse().unwrap());
        assert!(require_same_origin(&headers).is_ok());
        headers.insert("sec-fetch-site", "same-site".parse().unwrap());
        assert!(require_same_origin(&headers).is_err());
    }
}
