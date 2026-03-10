use gateway_core::StoreError;
use serde::Serialize;
use serde_json::{Map, Value};
use time::OffsetDateTime;
use uuid::Uuid;

pub(crate) fn parse_uuid(raw: &str) -> Result<Uuid, StoreError> {
    Uuid::parse_str(raw).map_err(|error| StoreError::Serialization(error.to_string()))
}

pub(crate) fn unix_to_datetime(ts: i64) -> Result<OffsetDateTime, StoreError> {
    OffsetDateTime::from_unix_timestamp(ts)
        .map_err(|error| StoreError::Serialization(error.to_string()))
}

pub(crate) fn json_object_from_str(value: &str) -> Result<Map<String, Value>, StoreError> {
    serde_json::from_str(value).map_err(|error| StoreError::Serialization(error.to_string()))
}

pub(crate) fn serialize_json<T>(value: &T) -> Result<String, StoreError>
where
    T: ?Sized + Serialize,
{
    serde_json::to_string(value).map_err(|error| StoreError::Serialization(error.to_string()))
}

pub(crate) fn serialize_optional_json<T>(value: Option<&T>) -> Result<Option<String>, StoreError>
where
    T: ?Sized + Serialize,
{
    value.map(serialize_json).transpose()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{json_object_from_str, parse_uuid, serialize_json, serialize_optional_json};

    #[test]
    fn serialize_helpers_round_trip_json_values() {
        let payload = json!({"provider": "openai", "timeout_ms": 120000});
        let encoded = serialize_json(&payload).expect("encode");
        let decoded = json_object_from_str(&encoded).expect("decode");

        assert_eq!(decoded.get("provider"), Some(&json!("openai")));
        assert_eq!(decoded.get("timeout_ms"), Some(&json!(120000)));
    }

    #[test]
    fn serialize_optional_json_handles_none() {
        assert_eq!(
            serialize_optional_json::<serde_json::Value>(None).expect("encode none"),
            None
        );
    }

    #[test]
    fn parse_uuid_rejects_invalid_values() {
        assert!(parse_uuid("not-a-uuid").is_err());
    }
}
