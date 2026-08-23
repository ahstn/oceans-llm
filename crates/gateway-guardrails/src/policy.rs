use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    FailureDisposition, GuardPhase, PolicyMode,
    packs::{BUILT_IN_PACK_IDS, PackId},
};

const DEFAULT_TIMEOUT_MS: u64 = 2_000;
const DEFAULT_MAX_CONTENT_BYTES: usize = 256 * 1024;
const DEFAULT_STREAM_BUFFER_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedCheckKind {
    AmazonBedrock,
    GoogleModelArmor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BedrockManagedAuthConfig {
    DefaultChain,
    StaticCredentials {
        access_key_id: String,
        secret_access_key: String,
        #[serde(default)]
        session_token: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BedrockManagedConfig {
    pub region: String,
    pub guardrail_identifier: String,
    pub guardrail_version: String,
    #[serde(default)]
    pub endpoint_url: Option<String>,
    pub auth: BedrockManagedAuthConfig,
    #[serde(default = "default_managed_retries")]
    pub max_retries: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelArmorAuthConfig {
    BearerToken { token: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelArmorManagedConfig {
    pub project: String,
    pub location: String,
    #[serde(default)]
    pub prompt_template: Option<String>,
    #[serde(default)]
    pub response_template: Option<String>,
    #[serde(default)]
    pub endpoint_url: Option<String>,
    pub auth: ModelArmorAuthConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedCheckConfig {
    pub kind: ManagedCheckKind,
    #[serde(default)]
    pub phases: BTreeSet<GuardPhase>,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub failure_disposition: FailureDisposition,
    #[serde(default = "default_max_content_bytes")]
    pub max_content_bytes: usize,
    #[serde(default)]
    pub bedrock: Option<BedrockManagedConfig>,
    #[serde(default)]
    pub model_armor: Option<ModelArmorManagedConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub mode: PolicyMode,
    #[serde(default)]
    pub packs: Vec<PackId>,
    #[serde(default)]
    pub managed_checks: Vec<String>,
    #[serde(default = "default_stream_buffer_bytes")]
    pub stream_buffer_bytes: usize,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: PolicyMode::Audit,
            packs: Vec::new(),
            managed_checks: Vec::new(),
            stream_buffer_bytes: default_stream_buffer_bytes(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyOverride {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub mode: Option<PolicyMode>,
    #[serde(default)]
    pub packs: Option<Vec<PackId>>,
    #[serde(default)]
    pub managed_checks: Option<Vec<String>>,
    #[serde(default)]
    pub stream_buffer_bytes: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuardrailConfig {
    #[serde(default)]
    pub default: PolicyConfig,
    #[serde(default)]
    pub managed_checks: BTreeMap<String, ManagedCheckConfig>,
    #[serde(default)]
    pub model_routes: BTreeMap<String, PolicyOverride>,
    #[serde(default)]
    pub mcp_servers: BTreeMap<String, PolicyOverride>,
}

impl GuardrailConfig {
    pub fn validate(
        &self,
        known_model_routes: &BTreeSet<String>,
        known_mcp_servers: &BTreeSet<String>,
    ) -> Result<(), GuardrailConfigError> {
        validate_policy("default", &self.default, &self.managed_checks)?;

        for (name, check) in &self.managed_checks {
            if name.trim().is_empty() {
                return Err(GuardrailConfigError::EmptyManagedCheckName);
            }
            if !(1..=120_000).contains(&check.timeout_ms) {
                return Err(GuardrailConfigError::InvalidTimeout {
                    check: name.clone(),
                    timeout_ms: check.timeout_ms,
                });
            }
            if check.max_content_bytes == 0 {
                return Err(GuardrailConfigError::InvalidContentLimit(name.clone()));
            }
            if check.phases.is_empty() {
                return Err(GuardrailConfigError::NoManagedPhases(name.clone()));
            }
            validate_managed_settings(name, check)?;
        }

        for (route, policy) in &self.model_routes {
            if !known_model_routes.contains(route) {
                return Err(GuardrailConfigError::UnknownModelRoute(route.clone()));
            }
            validate_override(
                &format!("model route `{route}`"),
                policy,
                &self.default,
                &self.managed_checks,
            )?;
        }
        for (server, policy) in &self.mcp_servers {
            if !known_mcp_servers.contains(server) {
                return Err(GuardrailConfigError::UnknownMcpServer(server.clone()));
            }
            validate_override(
                &format!("MCP server `{server}`"),
                policy,
                &self.default,
                &self.managed_checks,
            )?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectivePolicy {
    pub enabled: bool,
    pub mode: PolicyMode,
    pub packs: Vec<PackId>,
    pub managed_checks: Vec<String>,
    pub stream_buffer_bytes: usize,
    pub scope: crate::EffectiveScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyTarget<'a> {
    Global,
    ModelRoute(&'a str),
    McpServer(&'a str),
}

pub struct PolicyResolver<'a> {
    config: &'a GuardrailConfig,
}

impl<'a> PolicyResolver<'a> {
    pub fn new(config: &'a GuardrailConfig) -> Self {
        Self { config }
    }

    pub fn resolve(&self, target: PolicyTarget<'_>) -> EffectivePolicy {
        let (policy_override, scope) = match target {
            PolicyTarget::Global => (None, crate::EffectiveScope::Global),
            PolicyTarget::ModelRoute(route) => (
                self.config.model_routes.get(route),
                crate::EffectiveScope::ModelRoute(route.to_string()),
            ),
            PolicyTarget::McpServer(server) => (
                self.config.mcp_servers.get(server),
                crate::EffectiveScope::McpServer(server.to_string()),
            ),
        };
        let default = &self.config.default;
        EffectivePolicy {
            enabled: policy_override
                .and_then(|policy| policy.enabled)
                .unwrap_or(default.enabled),
            mode: policy_override
                .and_then(|policy| policy.mode)
                .unwrap_or(default.mode),
            packs: policy_override
                .and_then(|policy| policy.packs.clone())
                .unwrap_or_else(|| default.packs.clone()),
            managed_checks: policy_override
                .and_then(|policy| policy.managed_checks.clone())
                .unwrap_or_else(|| default.managed_checks.clone()),
            stream_buffer_bytes: policy_override
                .and_then(|policy| policy.stream_buffer_bytes)
                .unwrap_or(default.stream_buffer_bytes),
            scope,
        }
    }
}

fn validate_policy(
    label: &str,
    policy: &PolicyConfig,
    managed: &BTreeMap<String, ManagedCheckConfig>,
) -> Result<(), GuardrailConfigError> {
    validate_pack_ids(label, &policy.packs)?;
    validate_managed_references(label, &policy.managed_checks, managed)?;
    validate_stream_buffer(label, policy.stream_buffer_bytes)
}

fn validate_override(
    label: &str,
    policy: &PolicyOverride,
    default: &PolicyConfig,
    managed: &BTreeMap<String, ManagedCheckConfig>,
) -> Result<(), GuardrailConfigError> {
    if let Some(packs) = &policy.packs {
        validate_pack_ids(label, packs)?;
    }
    if let Some(checks) = &policy.managed_checks {
        validate_managed_references(label, checks, managed)?;
    }
    validate_stream_buffer(
        label,
        policy
            .stream_buffer_bytes
            .unwrap_or(default.stream_buffer_bytes),
    )
}

fn validate_pack_ids(label: &str, packs: &[PackId]) -> Result<(), GuardrailConfigError> {
    let mut seen = BTreeSet::new();
    for pack in packs {
        if !BUILT_IN_PACK_IDS.contains(&pack.as_str()) {
            return Err(GuardrailConfigError::UnknownPack(pack.to_string()));
        }
        if !seen.insert(pack) {
            return Err(GuardrailConfigError::DuplicatePack {
                policy: label.to_string(),
                pack: pack.to_string(),
            });
        }
    }
    Ok(())
}

fn validate_managed_references(
    label: &str,
    checks: &[String],
    managed: &BTreeMap<String, ManagedCheckConfig>,
) -> Result<(), GuardrailConfigError> {
    let mut seen = BTreeSet::new();
    for check in checks {
        if !managed.contains_key(check) {
            return Err(GuardrailConfigError::UnknownManagedCheck(check.clone()));
        }
        if !seen.insert(check) {
            return Err(GuardrailConfigError::DuplicateManagedCheck {
                policy: label.to_string(),
                check: check.clone(),
            });
        }
    }
    Ok(())
}

fn validate_managed_settings(
    name: &str,
    check: &ManagedCheckConfig,
) -> Result<(), GuardrailConfigError> {
    match (&check.kind, &check.bedrock, &check.model_armor) {
        (ManagedCheckKind::AmazonBedrock, Some(config), None) => {
            crate::managed::validate_guardrail_identifier(&config.guardrail_identifier).map_err(
                |error| GuardrailConfigError::InvalidManagedSettings {
                    check: name.to_string(),
                    message: error.to_string(),
                },
            )?;
            crate::managed::validate_guardrail_version(&config.guardrail_version).map_err(
                |error| GuardrailConfigError::InvalidManagedSettings {
                    check: name.to_string(),
                    message: error.to_string(),
                },
            )?;
            if config.region.trim().is_empty() {
                return Err(GuardrailConfigError::InvalidManagedSettings {
                    check: name.to_string(),
                    message: "region cannot be empty".to_string(),
                });
            }
        }
        (ManagedCheckKind::GoogleModelArmor, None, Some(config)) => {
            for template in [
                config.prompt_template.as_deref(),
                config.response_template.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                let (project, location) = crate::managed::validate_template_resource_name(template)
                    .map_err(|error| GuardrailConfigError::InvalidManagedSettings {
                        check: name.to_string(),
                        message: error.to_string(),
                    })?;
                if project != config.project || location != config.location {
                    return Err(GuardrailConfigError::InvalidManagedSettings {
                        check: name.to_string(),
                        message: "template project and location must match".to_string(),
                    });
                }
            }
            let needs_prompt_template = check.phases.iter().any(|phase| {
                matches!(
                    phase,
                    GuardPhase::Prompt | GuardPhase::McpCall | GuardPhase::HarnessPreTool
                )
            });
            if needs_prompt_template && config.prompt_template.is_none() {
                return Err(GuardrailConfigError::InvalidManagedSettings {
                    check: name.to_string(),
                    message: "selected prompt, MCP-call, or harness phase requires prompt_template"
                        .to_string(),
                });
            }
            let needs_response_template = check.phases.iter().any(|phase| {
                matches!(
                    phase,
                    GuardPhase::ModelResponse
                        | GuardPhase::GeneratedToolCall
                        | GuardPhase::McpResult
                )
            });
            if needs_response_template && config.response_template.is_none() {
                return Err(GuardrailConfigError::InvalidManagedSettings {
                    check: name.to_string(),
                    message:
                        "selected response, generated-tool-call, or MCP-result phase requires response_template"
                            .to_string(),
                });
            }
            if config.prompt_template.is_none() && config.response_template.is_none() {
                return Err(GuardrailConfigError::InvalidManagedSettings {
                    check: name.to_string(),
                    message: "a prompt or response template is required".to_string(),
                });
            }
        }
        _ => {
            return Err(GuardrailConfigError::InvalidManagedSettings {
                check: name.to_string(),
                message: "managed service settings do not match kind".to_string(),
            });
        }
    }
    Ok(())
}

fn validate_stream_buffer(label: &str, bytes: usize) -> Result<(), GuardrailConfigError> {
    if bytes == 0 {
        return Err(GuardrailConfigError::InvalidStreamBuffer(label.to_string()));
    }
    Ok(())
}

const fn default_timeout_ms() -> u64 {
    DEFAULT_TIMEOUT_MS
}

const fn default_max_content_bytes() -> usize {
    DEFAULT_MAX_CONTENT_BYTES
}

const fn default_stream_buffer_bytes() -> usize {
    DEFAULT_STREAM_BUFFER_BYTES
}

const fn default_managed_retries() -> u8 {
    1
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GuardrailConfigError {
    #[error("unknown built-in guardrail pack `{0}`")]
    UnknownPack(String),
    #[error("guardrail policy `{policy}` references pack `{pack}` more than once")]
    DuplicatePack { policy: String, pack: String },
    #[error("unknown managed guardrail check `{0}`")]
    UnknownManagedCheck(String),
    #[error("guardrail policy `{policy}` references managed check `{check}` more than once")]
    DuplicateManagedCheck { policy: String, check: String },
    #[error("managed guardrail check name cannot be empty")]
    EmptyManagedCheckName,
    #[error("managed guardrail check `{check}` timeout {timeout_ms}ms is outside 1..=120000")]
    InvalidTimeout { check: String, timeout_ms: u64 },
    #[error("managed guardrail check `{0}` must select at least one phase")]
    NoManagedPhases(String),
    #[error("managed guardrail check `{0}` content limit must be greater than zero")]
    InvalidContentLimit(String),
    #[error("unknown guardrail model-route override `{0}`")]
    UnknownModelRoute(String),
    #[error("unknown guardrail MCP-server override `{0}`")]
    UnknownMcpServer(String),
    #[error("guardrail policy `{0}` stream buffer must be greater than zero")]
    InvalidStreamBuffer(String),
    #[error("managed guardrail check `{check}` is invalid: {message}")]
    InvalidManagedSettings { check: String, message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(value: &str) -> PackId {
        PackId::new(value).unwrap()
    }

    #[test]
    fn route_and_server_overrides_replace_global_fields() {
        let config = GuardrailConfig {
            default: PolicyConfig {
                enabled: true,
                mode: PolicyMode::Audit,
                packs: vec![pack("core.shell")],
                managed_checks: Vec::new(),
                stream_buffer_bytes: 100,
            },
            model_routes: BTreeMap::from([(
                "openai/gpt".into(),
                PolicyOverride {
                    mode: Some(PolicyMode::Deny),
                    ..PolicyOverride::default()
                },
            )]),
            mcp_servers: BTreeMap::from([(
                "notion".into(),
                PolicyOverride {
                    packs: Some(vec![pack("saas.notion")]),
                    ..PolicyOverride::default()
                },
            )]),
            ..GuardrailConfig::default()
        };
        let resolver = PolicyResolver::new(&config);

        let route = resolver.resolve(PolicyTarget::ModelRoute("openai/gpt"));
        assert_eq!(route.mode, PolicyMode::Deny);
        assert_eq!(route.packs, vec![pack("core.shell")]);
        let server = resolver.resolve(PolicyTarget::McpServer("notion"));
        assert_eq!(server.mode, PolicyMode::Audit);
        assert_eq!(server.packs, vec![pack("saas.notion")]);
    }

    #[test]
    fn rejects_unknown_pack_and_override_reference() {
        let config = GuardrailConfig {
            default: PolicyConfig {
                packs: vec![pack("not.real")],
                ..PolicyConfig::default()
            },
            ..GuardrailConfig::default()
        };
        assert!(matches!(
            config.validate(&BTreeSet::new(), &BTreeSet::new()),
            Err(GuardrailConfigError::UnknownPack(_))
        ));

        let config = GuardrailConfig {
            model_routes: BTreeMap::from([("missing".into(), PolicyOverride::default())]),
            ..GuardrailConfig::default()
        };
        assert_eq!(
            config.validate(&BTreeSet::new(), &BTreeSet::new()),
            Err(GuardrailConfigError::UnknownModelRoute("missing".into()))
        );
    }

    #[test]
    fn rejects_duplicate_references_invalid_limits_and_mismatched_managed_settings() {
        let valid_bedrock = BedrockManagedConfig {
            region: "us-east-1".into(),
            guardrail_identifier: "guardrail-1".into(),
            guardrail_version: "1".into(),
            endpoint_url: None,
            auth: BedrockManagedAuthConfig::DefaultChain,
            max_retries: 1,
        };
        let managed = ManagedCheckConfig {
            kind: ManagedCheckKind::AmazonBedrock,
            phases: BTreeSet::from([GuardPhase::Prompt]),
            timeout_ms: 100,
            failure_disposition: FailureDisposition::FailOpen,
            max_content_bytes: 100,
            bedrock: Some(valid_bedrock),
            model_armor: None,
        };
        let config = GuardrailConfig {
            default: PolicyConfig {
                packs: vec![pack("core.shell"), pack("core.shell")],
                ..PolicyConfig::default()
            },
            managed_checks: BTreeMap::from([("aws".into(), managed.clone())]),
            ..GuardrailConfig::default()
        };
        assert!(matches!(
            config.validate(&BTreeSet::new(), &BTreeSet::new()),
            Err(GuardrailConfigError::DuplicatePack { .. })
        ));

        let config = GuardrailConfig {
            default: PolicyConfig {
                managed_checks: vec!["aws".into(), "aws".into()],
                ..PolicyConfig::default()
            },
            managed_checks: BTreeMap::from([("aws".into(), managed.clone())]),
            ..GuardrailConfig::default()
        };
        assert!(matches!(
            config.validate(&BTreeSet::new(), &BTreeSet::new()),
            Err(GuardrailConfigError::DuplicateManagedCheck { .. })
        ));

        for invalid in [
            ManagedCheckConfig {
                timeout_ms: 0,
                ..managed.clone()
            },
            ManagedCheckConfig {
                max_content_bytes: 0,
                ..managed.clone()
            },
            ManagedCheckConfig {
                phases: BTreeSet::new(),
                ..managed.clone()
            },
            ManagedCheckConfig {
                bedrock: None,
                ..managed.clone()
            },
        ] {
            let config = GuardrailConfig {
                managed_checks: BTreeMap::from([("aws".into(), invalid)]),
                ..GuardrailConfig::default()
            };
            assert!(config.validate(&BTreeSet::new(), &BTreeSet::new()).is_err());
        }

        let config = GuardrailConfig {
            default: PolicyConfig {
                stream_buffer_bytes: 0,
                ..PolicyConfig::default()
            },
            ..GuardrailConfig::default()
        };
        assert_eq!(
            config.validate(&BTreeSet::new(), &BTreeSet::new()),
            Err(GuardrailConfigError::InvalidStreamBuffer("default".into()))
        );

        let config = GuardrailConfig {
            mcp_servers: BTreeMap::from([("missing".into(), PolicyOverride::default())]),
            ..GuardrailConfig::default()
        };
        assert_eq!(
            config.validate(&BTreeSet::new(), &BTreeSet::new()),
            Err(GuardrailConfigError::UnknownMcpServer("missing".into()))
        );
    }

    #[test]
    fn rejects_model_armor_phases_without_the_required_template() {
        let managed = ManagedCheckConfig {
            kind: ManagedCheckKind::GoogleModelArmor,
            phases: BTreeSet::from([GuardPhase::ModelResponse]),
            timeout_ms: 100,
            failure_disposition: FailureDisposition::FailOpen,
            max_content_bytes: 100,
            bedrock: None,
            model_armor: Some(ModelArmorManagedConfig {
                project: "project".into(),
                location: "us-central1".into(),
                prompt_template: Some(
                    "projects/project/locations/us-central1/templates/prompt".into(),
                ),
                response_template: None,
                endpoint_url: None,
                auth: ModelArmorAuthConfig::BearerToken {
                    token: "test".into(),
                },
            }),
        };
        let config = GuardrailConfig {
            managed_checks: BTreeMap::from([("armor".into(), managed)]),
            ..GuardrailConfig::default()
        };

        assert!(matches!(
            config.validate(&BTreeSet::new(), &BTreeSet::new()),
            Err(GuardrailConfigError::InvalidManagedSettings { check, message })
                if check == "armor" && message.contains("response_template")
        ));
    }
}
