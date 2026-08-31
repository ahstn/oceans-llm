use crate::{MatchedRule, command::CommandInvocation};

use super::{has_option, rule, subcommand_index};

pub(super) fn match_invocation(invocation: &CommandInvocation) -> Option<MatchedRule> {
    if invocation.executable != "kubectl" {
        return None;
    }

    let arguments = &invocation.arguments;
    let operation_index = subcommand_index(arguments, KUBECTL_GLOBAL_OPTIONS_WITH_VALUES)?;
    let operation = arguments[operation_index].as_str();
    let command_arguments = &arguments[operation_index + 1..];
    let delete = operation == "delete";
    let dry_run = effective_dry_run(arguments);
    let (rule_id, reason_code, description, safer_action) = if delete
        && has_option(arguments, "--force", None)
        && !dry_run
    {
        (
            "delete-force",
            "kubectl.delete_force",
            "Removes resources without graceful shutdown",
            "Use the default grace period and inspect why the resource is stuck",
        )
    } else if delete && contains_any(command_arguments, &["namespace", "ns"]) && !dry_run {
        (
            "delete-namespace",
            "kubectl.delete_namespace",
            "Deletes a namespace and every resource in it",
            "List namespace resources and preview deletion with --dry-run=client",
        )
    } else if delete && has_option(arguments, "--all", None) && !dry_run {
        (
            "delete-all",
            "kubectl.delete_all",
            "Deletes every resource of the selected type",
            "Use a name or label selector and preview with --dry-run=client",
        )
    } else if delete && (has_option(arguments, "--all-namespaces", Some('A'))) && !dry_run {
        (
            "delete-all-namespaces",
            "kubectl.delete_all_namespaces",
            "Deletes matching resources across every namespace",
            "Select one namespace and preview with --dry-run=client",
        )
    } else if operation == "drain" && !dry_run {
        (
            "drain-node",
            "kubectl.drain_node",
            "Evicts workloads from a Kubernetes node",
            "Review pod disruption budgets and affected workloads before draining",
        )
    } else if operation == "cordon" && !dry_run {
        (
            "cordon-node",
            "kubectl.cordon_node",
            "Marks a node as unschedulable",
            "Review node capacity and workload placement before cordoning",
        )
    } else if operation == "taint"
        && command_arguments
            .iter()
            .any(|argument| argument.ends_with(":NoExecute"))
        && !dry_run
    {
        (
            "taint-noexecute",
            "kubectl.taint_noexecute",
            "Evicts pods that do not tolerate a NoExecute taint",
            "Review affected pods and tolerations before applying the taint",
        )
    } else if delete
        && contains_any(
            command_arguments,
            &[
                "deployment",
                "deploy",
                "statefulset",
                "sts",
                "daemonset",
                "ds",
                "replicaset",
                "rs",
            ],
        )
        && !dry_run
    {
        (
            "delete-workload",
            "kubectl.delete_workload",
            "Deletes a Kubernetes workload and its managed pods",
            "Preview with --dry-run=client or use a controlled rollout operation",
        )
    } else if delete
        && contains_any(command_arguments, &["pvc", "persistentvolumeclaim"])
        && !dry_run
    {
        (
            "delete-pvc",
            "kubectl.delete_pvc",
            "Deletes a persistent volume claim and may delete stored data",
            "Check the reclaim policy and active mounts before deletion",
        )
    } else if delete && contains_any(command_arguments, &["pv", "persistentvolume"]) && !dry_run {
        (
            "delete-pv",
            "kubectl.delete_pv",
            "Deletes a persistent volume and may delete underlying storage",
            "Check the reclaim policy and preserve required data first",
        )
    } else if operation == "scale" && replicas_are_zero(command_arguments) && !dry_run {
        (
            "scale-to-zero",
            "kubectl.scale_to_zero",
            "Stops every pod for the selected workload",
            "Review traffic and availability requirements before scaling down",
        )
    } else if operation == "apply" && has_option(command_arguments, "--prune", None) && !dry_run {
        (
            "apply-prune",
            "kubectl.apply_prune",
            "Deletes live resources absent from the applied configuration",
            "Run kubectl diff and review every object selected for pruning",
        )
    } else if operation == "apply" && has_option(command_arguments, "--force", None) && !dry_run {
        (
            "apply-force",
            "kubectl.apply_force",
            "Deletes and recreates resources during apply",
            "Use kubectl diff and apply without --force",
        )
    } else if delete && filename_is_stdin(command_arguments) && !dry_run {
        (
            "delete-from-stdin",
            "kubectl.delete_from_stdin",
            "Deletes resources from an agent-generated stdin manifest",
            "Save and review the manifest, then preview with --dry-run=client",
        )
    } else if delete && recursive_filename(command_arguments) && !dry_run {
        (
            "delete-from-directory",
            "kubectl.delete_from_directory",
            "Deletes every resource described by a directory tree",
            "Use a specific reviewed file and preview with --dry-run=client",
        )
    } else if operation == "replace" && has_option(command_arguments, "--force", None) && !dry_run {
        (
            "replace-force",
            "kubectl.replace_force",
            "Deletes and recreates resources during replace",
            "Review the manifest and replace without --force",
        )
    } else if operation == "auth"
        && command_arguments
            .first()
            .is_some_and(|value| value == "reconcile")
        && (has_option(command_arguments, "--remove-extra-permissions", None)
            || has_option(command_arguments, "--remove-extra-subjects", None))
        && !dry_run
    {
        (
            "auth-reconcile-remove",
            "kubectl.auth_reconcile_remove",
            "Removes RBAC permissions or subjects absent from the manifest",
            "Review the RBAC diff before removing access",
        )
    } else if delete && delete_has_target(command_arguments) && !dry_run {
        (
            "delete-resource",
            "kubectl.delete_resource",
            "Deletes one or more Kubernetes resources",
            "Preview the exact resources with --dry-run=client",
        )
    } else {
        return None;
    };

    Some(rule(
        "kubernetes.kubectl",
        rule_id,
        "command.arguments",
        reason_code,
        description,
        safer_action,
    ))
}

const KUBECTL_GLOBAL_OPTIONS_WITH_VALUES: &[&str] = &[
    "--as",
    "--as-group",
    "--as-uid",
    "--cache-dir",
    "--certificate-authority",
    "--client-certificate",
    "--client-key",
    "--cluster",
    "--context",
    "--kubeconfig",
    "--namespace",
    "-n",
    "--profile",
    "--profile-output",
    "--request-timeout",
    "--server",
    "-s",
    "--tls-server-name",
    "--token",
    "--user",
    "--v",
];

fn delete_has_target(arguments: &[String]) -> bool {
    arguments.iter().any(|argument| {
        !argument.starts_with('-')
            || matches!(
                argument.as_str(),
                "-f" | "--filename" | "-k" | "--kustomize" | "-l" | "--selector" | "--raw"
            )
            || argument.starts_with("--filename=")
            || argument.starts_with("--kustomize=")
            || argument.starts_with("--selector=")
            || argument.starts_with("--field-selector=")
            || argument.starts_with("--raw=")
    })
}

fn contains_any(arguments: &[String], values: &[&str]) -> bool {
    arguments
        .iter()
        .any(|argument| values.contains(&argument.as_str()))
}

fn replicas_are_zero(arguments: &[String]) -> bool {
    arguments.iter().any(|argument| argument == "--replicas=0")
        || arguments
            .windows(2)
            .any(|window| window[0] == "--replicas" && window[1] == "0")
}

fn filename_is_stdin(arguments: &[String]) -> bool {
    arguments
        .windows(2)
        .any(|window| matches!(window[0].as_str(), "-f" | "--filename") && window[1] == "-")
        || arguments.iter().any(|argument| {
            argument
                .strip_prefix("--filename=")
                .is_some_and(|value| value.split(',').any(|item| item == "-"))
                || argument
                    .strip_prefix("-f=")
                    .is_some_and(|value| value.split(',').any(|item| item == "-"))
        })
}

fn recursive_filename(arguments: &[String]) -> bool {
    has_option(arguments, "--recursive", Some('R'))
        || arguments.windows(2).any(|window| {
            matches!(window[0].as_str(), "-f" | "--filename")
                && (window[1] == "." || window[1].ends_with('/'))
        })
}

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
                matches!(value, "client" | "server" | "none" | "false" | "true")
            }) {
                effective = separated;
                index += 1;
            } else {
                effective = Some("true");
            }
        }
        index += 1;
    }
    effective.is_some_and(|value| matches!(value, "client" | "server" | "true"))
}
