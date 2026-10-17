//! Keyed config collections (`providers`, `models`) accept either a list of entries with an
//! `id` field or a map from id to entry. Maps compose across Helm values files and config
//! fragments; lists are kept for existing configs.

use anyhow::bail;
use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use serde_yaml::{Mapping, Value};

/// Returns `(id, entry)` pairs in document order, with `id` set inside each entry.
pub(super) fn keyed_entries(
    value: Value,
    collection: &str,
) -> anyhow::Result<Vec<(String, Value)>> {
    match value {
        Value::Null => Ok(Vec::new()),
        Value::Sequence(items) => items
            .into_iter()
            .map(|item| {
                let id = item
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .ok_or_else(|| anyhow::anyhow!("{collection} list entries require an `id`"))?;
                Ok((id, item))
            })
            .collect(),
        Value::Mapping(entries) => entries
            .into_iter()
            .map(|(key, entry)| {
                let Value::String(id) = key else {
                    bail!("{collection} map keys must be strings");
                };
                let mut fields = match entry {
                    Value::Mapping(fields) => fields,
                    Value::Null => Mapping::new(),
                    _ => bail!("{collection} `{id}` must be a mapping"),
                };
                if fields.contains_key("id") {
                    bail!("{collection} `{id}` must not set `id`; the map key is the id");
                }
                let mut with_id = Mapping::with_capacity(fields.len() + 1);
                with_id.insert(Value::String("id".to_string()), Value::String(id.clone()));
                with_id.extend(std::mem::take(&mut fields));
                Ok((id, Value::Mapping(with_id)))
            })
            .collect(),
        _ => bail!("{collection} must be a list or a map keyed by id"),
    }
}

fn deserialize_keyed<'de, D, T>(deserializer: D, collection: &str) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let value = Value::deserialize(deserializer)?;
    keyed_entries(value, collection)
        .map_err(serde::de::Error::custom)?
        .into_iter()
        .map(|(id, entry)| {
            // Entries are re-read from a buffered value, which drops line numbers, so name them.
            serde_yaml::from_value(entry)
                .map_err(|error| serde::de::Error::custom(format!("{collection} `{id}`: {error}")))
        })
        .collect()
}

pub(super) fn deserialize_providers<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    deserialize_keyed(deserializer, "providers")
}

pub(super) fn deserialize_models<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    deserialize_keyed(deserializer, "models")
}
