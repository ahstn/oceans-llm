//! Byte-stable hashes shared by passive session analysis and attribution.

use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Groups a session window by the canonical application tags, independent of tag order.
pub fn boundary_group_key<'a>(
    service: Option<&str>,
    component: Option<&str>,
    environment: Option<&str>,
    bespoke: impl Iterator<Item = (&'a str, &'a str)>,
) -> String {
    let mut bespoke = bespoke.collect::<Vec<_>>();
    bespoke.sort_unstable();
    hash_identifier(
        &json!({
            "service": service,
            "component": component,
            "environment": environment,
            "bespoke": bespoke,
        })
        .to_string(),
    )
}
pub fn stable_uuid(namespace: Uuid, canonical: &str) -> Uuid {
    Uuid::new_v5(&namespace, canonical.as_bytes())
}

pub fn hash_identifier(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    format!("sha256:{digest:x}")
}

pub fn hash_lineage_candidate(
    ownership_scope_key: &str,
    adapter_namespace: &str,
    value: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(ownership_scope_key.as_bytes());
    hasher.update([0]);
    hasher.update(adapter_namespace.as_bytes());
    hasher.update([0]);
    hasher.update(b"lineage-v1");
    hasher.update([0]);
    hasher.update(value.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lineage_hash_retains_owner_namespace_and_version_encoding() {
        assert_eq!(
            hash_lineage_candidate("user:a", "codex", "raw-thread"),
            "sha256:442976352919be9141d21d95adbed4fb888f3c8255f53e8618edff7d67527d32",
        );
        assert_ne!(
            hash_lineage_candidate("user:a", "codex", "raw-thread"),
            hash_lineage_candidate("user:a", "claude_code", "raw-thread"),
        );
        assert_ne!(
            hash_lineage_candidate("user:a", "codex", "raw-thread"),
            hash_lineage_candidate("user:b", "codex", "raw-thread"),
        );
    }

    #[test]
    fn session_boundary_group_is_order_independent_and_byte_stable() {
        let bespoke = [("region", "east"), ("workflow", "review")];
        let key = boundary_group_key(Some("api"), None, None, bespoke.into_iter());
        assert_eq!(
            key,
            "sha256:8798ffefdeb8388c0bf47e312ca1de52d2e084afcc75b1ceba29504b3c37494d"
        );
        assert_eq!(
            key,
            boundary_group_key(Some("api"), None, None, bespoke.into_iter().rev()),
        );
        assert_ne!(
            key,
            boundary_group_key(Some("different"), None, None, bespoke.into_iter()),
        );
    }
}
