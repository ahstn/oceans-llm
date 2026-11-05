//! Tri-state route capabilities: explicit config, compatibility inference, catalog hints,
//! defaults, then the adapter ceiling.

use serde::{Deserialize, Serialize};

use crate::{
    AwsBedrockApiStyle, ModelRoute, ProviderCapabilities, RouteCompatibility,
    github_copilot_route_capabilities,
};

/// Route capability flags where `None` means "not decided by this source".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PartialCapabilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_completions: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub responses: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embeddings: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decisions: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vision: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_schema: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub developer_role: Option<bool>,
}

impl PartialCapabilities {
    /// Fields set in `self` win; unset fields take `fallback`'s value.
    #[must_use]
    pub const fn or(self, fallback: Self) -> Self {
        const fn pick(value: Option<bool>, fallback: Option<bool>) -> Option<bool> {
            match value {
                Some(value) => Some(value),
                None => fallback,
            }
        }
        Self {
            chat_completions: pick(self.chat_completions, fallback.chat_completions),
            responses: pick(self.responses, fallback.responses),
            stream: pick(self.stream, fallback.stream),
            embeddings: pick(self.embeddings, fallback.embeddings),
            images: pick(self.images, fallback.images),
            decisions: pick(self.decisions, fallback.decisions),
            tools: pick(self.tools, fallback.tools),
            vision: pick(self.vision, fallback.vision),
            json_schema: pick(self.json_schema, fallback.json_schema),
            developer_role: pick(self.developer_role, fallback.developer_role),
        }
    }

    /// Unset fields take the route defaults: `images` and `decisions` are opt-in, every
    /// other field is on.
    #[must_use]
    pub const fn resolve(self) -> ProviderCapabilities {
        const fn or_default(value: Option<bool>, default: bool) -> bool {
            match value {
                Some(value) => value,
                None => default,
            }
        }
        ProviderCapabilities {
            chat_completions: or_default(self.chat_completions, true),
            responses: or_default(self.responses, true),
            stream: or_default(self.stream, true),
            embeddings: or_default(self.embeddings, true),
            images: or_default(self.images, false),
            decisions: or_default(self.decisions, false),
            tools: or_default(self.tools, true),
            vision: or_default(self.vision, true),
            json_schema: or_default(self.json_schema, true),
            developer_role: or_default(self.developer_role, true),
        }
    }

    /// Every field set from a full capability set.
    #[must_use]
    pub const fn from_capabilities(caps: ProviderCapabilities) -> Self {
        Self {
            chat_completions: Some(caps.chat_completions),
            responses: Some(caps.responses),
            stream: Some(caps.stream),
            embeddings: Some(caps.embeddings),
            images: Some(caps.images),
            decisions: Some(caps.decisions),
            tools: Some(caps.tools),
            vision: Some(caps.vision),
            json_schema: Some(caps.json_schema),
            developer_role: Some(caps.developer_role),
        }
    }
}

impl From<ProviderCapabilities> for PartialCapabilities {
    fn from(caps: ProviderCapabilities) -> Self {
        Self::from_capabilities(caps)
    }
}

/// Capabilities the gateway adapter guarantees for a route's `compatibility` on a provider of
/// type `provider_type` (`ProviderConnection.provider_type`). Model-dependent fields stay unset.
#[must_use]
pub fn infer_route_capabilities(route: &ModelRoute, provider_type: &str) -> PartialCapabilities {
    infer_compatibility_capabilities(&route.compatibility, provider_type)
}

/// [`infer_route_capabilities`] for callers that hold only the route compatibility, such as
/// config validation before a `ModelRoute` exists.
#[must_use]
pub fn infer_compatibility_capabilities(
    compatibility: &RouteCompatibility,
    provider_type: &str,
) -> PartialCapabilities {
    match provider_type {
        "aws_bedrock" => compatibility
            .aws_bedrock
            .as_ref()
            .map(|bedrock| bedrock_capabilities(bedrock.api_style))
            .unwrap_or_default(),
        "github_copilot" => compatibility
            .github_copilot
            .as_ref()
            .map(|copilot| {
                PartialCapabilities::from_capabilities(github_copilot_route_capabilities(Some(
                    copilot,
                )))
            })
            .unwrap_or_default(),
        "openai_compat" if compatibility.selects_decisions_api() => decisions_only(),
        "typesafe" => decisions_only(),
        _ => PartialCapabilities::default(),
    }
}

/// The capabilities a route actually serves: explicit config, then compatibility inference,
/// then catalog `hints`, then defaults, capped by the `adapter` ceiling.
#[must_use]
pub fn effective_route_capabilities(
    route: &ModelRoute,
    provider_type: &str,
    hints: PartialCapabilities,
    adapter: ProviderCapabilities,
) -> ProviderCapabilities {
    route
        .capabilities
        .or(infer_route_capabilities(route, provider_type))
        .or(hints)
        .resolve()
        .intersect(adapter)
}

const fn bedrock_capabilities(api_style: AwsBedrockApiStyle) -> PartialCapabilities {
    let not_served = PartialCapabilities {
        embeddings: Some(false),
        images: Some(false),
        decisions: Some(false),
        ..empty()
    };
    match api_style {
        AwsBedrockApiStyle::MantleOpenaiResponses => PartialCapabilities {
            chat_completions: Some(false),
            responses: Some(true),
            stream: Some(true),
            ..not_served
        },
        AwsBedrockApiStyle::RuntimeAnthropicInvoke => PartialCapabilities {
            chat_completions: Some(true),
            responses: Some(false),
            stream: Some(false),
            json_schema: Some(false),
            ..not_served
        },
        AwsBedrockApiStyle::RuntimeConverse
        | AwsBedrockApiStyle::RuntimeOpenaiChat
        | AwsBedrockApiStyle::MantleOpenaiChat
        | AwsBedrockApiStyle::MantleAnthropicMessages => PartialCapabilities {
            chat_completions: Some(true),
            responses: Some(false),
            stream: Some(true),
            json_schema: Some(false),
            ..not_served
        },
    }
}

const fn decisions_only() -> PartialCapabilities {
    PartialCapabilities::from_capabilities(ProviderCapabilities {
        decisions: true,
        ..ProviderCapabilities::none()
    })
}

const fn empty() -> PartialCapabilities {
    PartialCapabilities {
        chat_completions: None,
        responses: None,
        stream: None,
        embeddings: None,
        images: None,
        decisions: None,
        tools: None,
        vision: None,
        json_schema: None,
        developer_role: None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uuid::Uuid;

    use super::{PartialCapabilities, effective_route_capabilities, infer_route_capabilities};
    use crate::{
        AwsBedrockApiStyle, AwsBedrockRouteCompatibility, ModelRoute, ProviderCapabilities,
        RouteCompatibility,
    };

    fn route(capabilities: PartialCapabilities, compatibility: RouteCompatibility) -> ModelRoute {
        ModelRoute {
            id: Uuid::nil(),
            model_id: Uuid::nil(),
            provider_key: "provider".to_string(),
            upstream_model: "model".to_string(),
            priority: 100,
            weight: 1.0,
            enabled: true,
            context_window_tokens: None,
            pricing_override: None,
            extra_headers: Default::default(),
            extra_body: Default::default(),
            capabilities,
            compatibility,
        }
    }

    fn converse() -> RouteCompatibility {
        RouteCompatibility {
            aws_bedrock: Some(AwsBedrockRouteCompatibility {
                api_style: AwsBedrockApiStyle::RuntimeConverse,
                openai_base_path: None,
                supports_strict_tools: None,
            }),
            ..RouteCompatibility::default()
        }
    }

    #[test]
    fn resolve_defaults_images_and_decisions_off() {
        assert_eq!(
            PartialCapabilities::default().resolve(),
            ProviderCapabilities {
                images: false,
                decisions: false,
                ..ProviderCapabilities::all_enabled()
            }
        );
    }

    #[test]
    fn explicit_false_beats_inferred_true() {
        let explicit = PartialCapabilities {
            stream: Some(false),
            ..PartialCapabilities::default()
        };
        let route = route(explicit, converse());
        assert_eq!(
            infer_route_capabilities(&route, "aws_bedrock").stream,
            Some(true)
        );

        let effective = effective_route_capabilities(
            &route,
            "aws_bedrock",
            PartialCapabilities::default(),
            ProviderCapabilities::all_enabled(),
        );
        assert!(!effective.stream);
        assert!(effective.chat_completions);
        assert!(!effective.responses, "inferred false fills the unset field");
    }

    #[test]
    fn explicit_true_is_kept_when_hints_say_false() {
        let route = route(
            PartialCapabilities {
                tools: Some(true),
                ..PartialCapabilities::default()
            },
            RouteCompatibility::default(),
        );
        let hints = PartialCapabilities {
            tools: Some(false),
            vision: Some(false),
            ..PartialCapabilities::default()
        };

        let effective = effective_route_capabilities(
            &route,
            "openai_compat",
            hints,
            ProviderCapabilities::all_enabled(),
        );
        assert!(effective.tools);
        assert!(!effective.vision, "hints fill unset fields");
    }

    #[test]
    fn inference_beats_hints_and_adapter_caps_everything() {
        let route = route(PartialCapabilities::default(), converse());
        let hints = PartialCapabilities::from_capabilities(ProviderCapabilities::all_enabled());
        let adapter = ProviderCapabilities {
            tools: false,
            ..ProviderCapabilities::all_enabled()
        };

        let effective = effective_route_capabilities(&route, "aws_bedrock", hints, adapter);
        assert!(!effective.responses);
        assert!(!effective.json_schema);
        assert!(
            effective.vision,
            "hint fills a field inference leaves unset"
        );
        assert!(!effective.tools, "adapter ceiling applies last");
    }

    #[test]
    fn inference_requires_matching_provider_type() {
        let route = route(PartialCapabilities::default(), converse());
        assert_eq!(
            infer_route_capabilities(&route, "openai_compat"),
            PartialCapabilities::default()
        );
    }

    #[test]
    fn serializes_only_set_fields_and_decodes_full_booleans() {
        let partial = PartialCapabilities {
            stream: Some(false),
            tools: Some(true),
            ..PartialCapabilities::default()
        };
        assert_eq!(
            serde_json::to_value(partial).expect("serialize"),
            json!({"stream": false, "tools": true})
        );

        let full = serde_json::to_value(ProviderCapabilities::all_enabled()).expect("serialize");
        let decoded: PartialCapabilities = serde_json::from_value(full).expect("decode");
        assert_eq!(
            decoded,
            PartialCapabilities::from_capabilities(ProviderCapabilities::all_enabled())
        );
    }
}
