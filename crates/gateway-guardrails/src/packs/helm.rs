use crate::{MatchedRule, command::CommandInvocation};

use super::{has_option, rule, subcommand_index};

pub(super) fn match_invocation(invocation: &CommandInvocation) -> Option<MatchedRule> {
    if invocation.executable != "helm" {
        return None;
    }

    let arguments = &invocation.arguments;
    if effective_dry_run(arguments) {
        return None;
    }
    let operation = arguments
        .get(subcommand_index(
            arguments,
            HELM_GLOBAL_OPTIONS_WITH_VALUES,
        )?)?
        .as_str();

    let (rule_id, reason_code, description, safer_action) = match operation {
        "uninstall" | "delete" | "del" | "un" => (
            "uninstall",
            "helm.uninstall",
            "Removes a Helm release and its Kubernetes resources",
            "Run helm uninstall --dry-run and inspect helm get all first",
        ),
        "rollback" => (
            "rollback",
            "helm.rollback",
            "Reverts release resources and values to an earlier revision",
            "Inspect helm history and preview the rollback with --dry-run",
        ),
        "upgrade"
            if has_option(arguments, "--force", None)
                || has_option(arguments, "--force-replace", None) =>
        {
            (
                "upgrade-force",
                "helm.upgrade_force",
                "Deletes and recreates resources during a Helm upgrade",
                "Remove force replacement and preview the upgrade with --dry-run",
            )
        }
        "upgrade" if has_option(arguments, "--reset-values", None) => (
            "upgrade-reset-values",
            "helm.upgrade_reset_values",
            "Discards values saved by earlier Helm releases",
            "Review helm get values and provide a complete values file",
        ),
        "install" | "upgrade"
            if has_option(arguments, "--cleanup-on-fail", None)
                || has_option(arguments, "--rollback-on-failure", None) =>
        {
            (
                "cleanup-on-failure",
                "helm.cleanup_on_failure",
                "Deletes newly created resources or uninstalls a failed release",
                "Inspect failure behavior and clean up reviewed resources explicitly",
            )
        }
        _ => return None,
    };

    Some(rule(
        "kubernetes.helm",
        rule_id,
        "command.arguments",
        reason_code,
        description,
        safer_action,
    ))
}
const HELM_GLOBAL_OPTIONS_WITH_VALUES: &[&str] = &[
    "--burst-limit",
    "--kube-apiserver",
    "--kube-as-group",
    "--kube-as-user",
    "--kube-ca-file",
    "--kube-context",
    "--kube-token",
    "--kubeconfig",
    "--namespace",
    "-n",
    "--qps",
    "--registry-config",
    "--repository-cache",
    "--repository-config",
];

fn effective_dry_run(arguments: &[String]) -> bool {
    let mut effective = None;
    let mut index = 0;
    while index < arguments.len() {
        let argument = &arguments[index];
        if argument == "--" {
            break;
        }
        if let Some(value) = argument.strip_prefix("--dry-run=") {
            effective = Some(value);
        } else if argument == "--dry-run" {
            let separated = arguments.get(index + 1).map(String::as_str);
            if separated.is_some_and(|value| {
                matches!(value, "true" | "false" | "none" | "client" | "server")
            }) {
                effective = separated;
                index += 1;
            } else {
                effective = Some("true");
            }
        }
        index += 1;
    }
    effective.is_some_and(|value| matches!(value, "true" | "client" | "server"))
}
