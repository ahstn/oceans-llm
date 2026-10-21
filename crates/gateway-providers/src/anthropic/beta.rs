use std::collections::BTreeMap;

use gateway_core::{ProviderError, ProviderRequestContext};
use serde_json::{Map, Value};

/// Only the capability header is caller-controlled; credentials and transport headers stay local.
/// Beta names remain an open list, with support and access validated by the upstream API.
pub(crate) fn header_betas(
    context: &ProviderRequestContext,
    defaults: Option<&BTreeMap<String, String>>,
) -> Vec<String> {
    let values = defaults
        .into_iter()
        .flat_map(|headers| headers.iter().map(|(name, value)| (name, value.as_str())))
        .chain(
            context
                .extra_headers
                .iter()
                .filter_map(|(name, value)| value.as_str().map(|value| (name, value))),
        )
        .chain(
            context
                .request_headers
                .iter()
                .map(|(name, value)| (name, value.as_str())),
        )
        .filter(|(name, _)| name.eq_ignore_ascii_case("anthropic-beta"))
        .flat_map(|(_, value)| value.split(',').map(str::trim))
        .filter(|beta| !beta.is_empty());
    let mut betas = Vec::new();
    for value in values {
        push_unique(&mut betas, value);
    }
    betas
}

/// Collect legacy body opt-ins together with configured and caller headers before choosing the
/// upstream dialect: a single header for Messages/Vertex, or a body array for InvokeModel.
pub(crate) fn take_request_betas(
    body: &mut Value,
    context: &ProviderRequestContext,
    defaults: &BTreeMap<String, String>,
) -> Result<Vec<String>, ProviderError> {
    let body_value = body
        .as_object_mut()
        .and_then(|body| body.remove("anthropic_beta"));
    let mut betas = body_betas(body_value.as_ref())?;
    for beta in header_betas(context, Some(defaults)) {
        push_unique(&mut betas, &beta);
    }
    Ok(betas)
}

pub(crate) fn body_betas(value: Option<&Value>) -> Result<Vec<String>, ProviderError> {
    let mut betas = Vec::new();
    match value {
        None | Some(Value::Null) => {}
        Some(Value::Array(values)) => {
            for value in values {
                let beta = value.as_str().ok_or_else(invalid_body_betas)?;
                push_unique(&mut betas, beta);
            }
        }
        Some(Value::String(value)) => {
            for beta in value
                .split(',')
                .map(str::trim)
                .filter(|beta| !beta.is_empty())
            {
                push_unique(&mut betas, beta);
            }
        }
        Some(_) => return Err(invalid_body_betas()),
    }
    Ok(betas)
}

pub(crate) fn put_body_betas(body: &mut Map<String, Value>, betas: Vec<String>) {
    if !betas.is_empty() {
        body.insert("anthropic_beta".to_string(), Value::from(betas));
    }
}

fn invalid_body_betas() -> ProviderError {
    ProviderError::InvalidRequest("`anthropic_beta` must be an array of strings".to_string())
}

fn push_unique(betas: &mut Vec<String>, beta: &str) {
    if !betas.iter().any(|existing| existing == beta) {
        betas.push(beta.to_string());
    }
}
