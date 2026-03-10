use uuid::Uuid;

pub(crate) fn model_uuid(model_key: &str) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("model:{model_key}").as_bytes(),
    )
}

pub(crate) fn route_uuid(
    model_key: &str,
    provider_key: &str,
    upstream_model: &str,
    priority: i32,
    route_index: usize,
) -> Uuid {
    let key = format!("route:{model_key}:{provider_key}:{upstream_model}:{priority}:{route_index}");
    Uuid::new_v5(&Uuid::NAMESPACE_OID, key.as_bytes())
}

pub(crate) fn api_key_uuid(public_id: &str) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("api_key:{public_id}").as_bytes(),
    )
}
