use serde::Serialize;
use serde_json::Value;

pub(crate) fn to_pretty_json(value: &Value) -> String {
    serde_json::to_string_pretty(value).expect("client config JSON should serialize")
}

pub(crate) fn to_pretty_toml<T: Serialize>(value: &T) -> String {
    toml::to_string_pretty(value).expect("client config TOML should serialize")
}
