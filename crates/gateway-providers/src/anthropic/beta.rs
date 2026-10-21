use std::collections::{BTreeMap, HashSet};

use gateway_core::{ProviderError, ProviderRequestContext};
use serde_json::{Map, Value};

// Gateway resource limits, not Anthropic feature-name restrictions. They leave room for future
// capabilities while bounding metadata work independently of the 64 MiB inference body limit.
pub(crate) const MAX_BETA_NAMES: usize = 1_024;
pub(crate) const MAX_BETA_NAME_BYTES: usize = 256;
pub(crate) const MAX_BETA_LIST_BYTES: usize = 256 * 1_024;

/// A hash set bounds duplicate checks to expected linear work; the vector retains first-seen order.
/// Borrow names until validation succeeds so oversized inputs do not allocate owned name copies.
#[derive(Default)]
struct OrderedBetas<'a> {
    names: Vec<&'a str>,
    seen: HashSet<&'a str>,
}

impl<'a> OrderedBetas<'a> {
    fn insert(&mut self, beta: &'a str) -> Result<(), ProviderError> {
        if beta.len() > MAX_BETA_NAME_BYTES {
            return Err(limit_error("name byte length", MAX_BETA_NAME_BYTES));
        }
        if self.seen.contains(beta) {
            return Ok(());
        }
        if self.names.len() == MAX_BETA_NAMES {
            return Err(limit_error("distinct name count", MAX_BETA_NAMES));
        }
        self.seen.insert(beta);
        self.names.push(beta);
        Ok(())
    }

    fn add_list(&mut self, value: &'a str) -> Result<(), ProviderError> {
        if value.len() > MAX_BETA_LIST_BYTES {
            return Err(limit_error(
                "comma-separated list byte length",
                MAX_BETA_LIST_BYTES,
            ));
        }
        for beta in value
            .split(',')
            .map(str::trim)
            .filter(|beta| !beta.is_empty())
        {
            self.insert(beta)?;
        }
        Ok(())
    }

    fn add_body(&mut self, value: Option<&'a Value>) -> Result<(), ProviderError> {
        match value {
            None | Some(Value::Null) => {}
            Some(Value::Array(values)) => {
                // Count before deduplication so repeated names cannot hide an oversized array.
                if values.len() > MAX_BETA_NAMES {
                    return Err(limit_error("body array entry count", MAX_BETA_NAMES));
                }
                for value in values {
                    let beta = value.as_str().ok_or_else(invalid_body_betas)?;
                    self.insert(beta)?;
                }
            }
            Some(Value::String(value)) => self.add_list(value)?,
            Some(_) => return Err(invalid_body_betas()),
        }
        Ok(())
    }

    fn into_strings(self) -> Vec<String> {
        self.names.into_iter().map(str::to_string).collect()
    }
}

fn collect_betas<'a>(
    body: Option<&'a Value>,
    context: &'a ProviderRequestContext,
    defaults: Option<&'a BTreeMap<String, String>>,
    additional: &[&'a str],
) -> Result<OrderedBetas<'a>, ProviderError> {
    let mut betas = OrderedBetas::default();
    betas.add_body(body)?;
    // Only this capability header is caller-controlled; feature names remain an open list.
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
        .filter(|(name, _)| name.eq_ignore_ascii_case("anthropic-beta"));
    let mut header_bytes = 0usize;
    for (_, value) in values {
        header_bytes = header_bytes.saturating_add(value.len());
        if header_bytes > MAX_BETA_LIST_BYTES {
            return Err(limit_error(
                "combined header byte length",
                MAX_BETA_LIST_BYTES,
            ));
        }
        betas.add_list(value)?;
    }
    for beta in additional {
        betas.insert(beta)?;
    }
    Ok(betas)
}

/// Validate and merge every opt-in source before choosing the provider's header/body dialect.
pub(crate) fn merged_request_betas(
    body: Option<&Value>,
    context: &ProviderRequestContext,
    defaults: Option<&BTreeMap<String, String>>,
    additional: &[&str],
) -> Result<Vec<String>, ProviderError> {
    collect_betas(body, context, defaults, additional).map(OrderedBetas::into_strings)
}

/// Validate the entire input before checking a feature, without allocating owned name copies.
pub(crate) fn request_has_beta(
    body: Option<&Value>,
    context: &ProviderRequestContext,
    defaults: Option<&BTreeMap<String, String>>,
    beta: &str,
) -> Result<bool, ProviderError> {
    collect_betas(body, context, defaults, &[]).map(|betas| betas.seen.contains(beta))
}

pub(crate) fn validate_request_betas(
    body: Option<&Value>,
    context: &ProviderRequestContext,
    defaults: Option<&BTreeMap<String, String>>,
) -> Result<(), ProviderError> {
    collect_betas(body, context, defaults, &[]).map(|_| ())
}

/// Normalize only body entries before a model adds its beta, without moving header entries
/// ahead of provider defaults that are merged later by the HTTP transport.
pub(crate) fn normalized_body_betas(body: Option<&Value>) -> Result<Vec<String>, ProviderError> {
    let mut betas = OrderedBetas::default();
    betas.add_body(body)?;
    Ok(betas.into_strings())
}

pub(crate) fn take_request_betas(
    body: &mut Value,
    context: &ProviderRequestContext,
    defaults: &BTreeMap<String, String>,
) -> Result<Vec<String>, ProviderError> {
    let body_value = body
        .as_object_mut()
        .and_then(|body| body.remove("anthropic_beta"));
    merged_request_betas(body_value.as_ref(), context, Some(defaults), &[])
}

pub(crate) fn put_body_betas(body: &mut Map<String, Value>, betas: Vec<String>) {
    if !betas.is_empty() {
        body.insert("anthropic_beta".to_string(), Value::from(betas));
    }
}

fn invalid_body_betas() -> ProviderError {
    ProviderError::InvalidRequest("`anthropic_beta` must be an array of strings".to_string())
}

fn limit_error(dimension: &str, limit: usize) -> ProviderError {
    ProviderError::InvalidRequest(format!(
        "Anthropic beta metadata {dimension} exceeds the gateway limit of {limit} (`anthropic-beta` / `anthropic_beta`)"
    ))
}
