use crate::{MatchedRule, command::CommandInvocation};

use super::{has_option, rule};

pub(super) fn match_invocation(invocation: &CommandInvocation) -> Option<MatchedRule> {
    if invocation.executable != "kubectl" {
        return None;
    }

    let arguments = &invocation.arguments;
    let delete = arguments.iter().any(|argument| argument == "delete");
    let dry_run = effective_dry_run(arguments);
    let (rule_id, reason_code, description, safer_action) = if delete
        && contains_any(arguments, &["namespace", "ns"])
        && !dry_run
    {
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
    } else if arguments.iter().any(|argument| argument == "drain") {
        (
            "drain-node",
            "kubectl.drain_node",
            "Evicts workloads from a Kubernetes node",
            "Review pod disruption budgets and affected workloads before draining",
        )
    } else if arguments.iter().any(|argument| argument == "cordon") {
        (
            "cordon-node",
            "kubectl.cordon_node",
            "Marks a node as unschedulable",
            "Review node capacity and workload placement before cordoning",
        )
    } else if arguments.iter().any(|argument| argument == "taint")
        && arguments
            .iter()
            .any(|argument| argument.ends_with(":NoExecute"))
    {
        (
            "taint-noexecute",
            "kubectl.taint_noexecute",
            "Evicts pods that do not tolerate a NoExecute taint",
            "Review affected pods and tolerations before applying the taint",
        )
    } else if delete
        && contains_any(
            arguments,
            &["deployment", "statefulset", "daemonset", "replicaset"],
        )
        && !dry_run
    {
        (
            "delete-workload",
            "kubectl.delete_workload",
            "Deletes a Kubernetes workload and its managed pods",
            "Preview with --dry-run=client or use a controlled rollout operation",
        )
    } else if delete && contains_any(arguments, &["pvc", "persistentvolumeclaim"]) && !dry_run {
        (
            "delete-pvc",
            "kubectl.delete_pvc",
            "Deletes a persistent volume claim and may delete stored data",
            "Check the reclaim policy and active mounts before deletion",
        )
    } else if delete && contains_any(arguments, &["pv", "persistentvolume"]) && !dry_run {
        (
            "delete-pv",
            "kubectl.delete_pv",
            "Deletes a persistent volume and may delete underlying storage",
            "Check the reclaim policy and preserve required data first",
        )
    } else if arguments.iter().any(|argument| argument == "scale") && replicas_are_zero(arguments) {
        (
            "scale-to-zero",
            "kubectl.scale_to_zero",
            "Stops every pod for the selected workload",
            "Review traffic and availability requirements before scaling down",
        )
    } else if delete
        && has_option(arguments, "--force", None)
        && option_is_zero(arguments, "--grace-period")
        && !dry_run
    {
        (
            "delete-force",
            "kubectl.delete_force",
            "Removes resources without graceful shutdown",
            "Use the default grace period and inspect why the resource is stuck",
        )
    } else if arguments.iter().any(|argument| argument == "apply")
        && has_option(arguments, "--force", None)
        && !dry_run
    {
        (
            "apply-force",
            "kubectl.apply_force",
            "Deletes and recreates resources during apply",
            "Use kubectl diff and apply without --force",
        )
    } else if delete && filename_is_stdin(arguments) && !dry_run {
        (
            "delete-from-stdin",
            "kubectl.delete_from_stdin",
            "Deletes resources from an agent-generated stdin manifest",
            "Save and review the manifest, then preview with --dry-run=client",
        )
    } else if delete && recursive_filename(arguments) && !dry_run {
        (
            "delete-from-directory",
            "kubectl.delete_from_directory",
            "Deletes every resource described by a directory tree",
            "Use a specific reviewed file and preview with --dry-run=client",
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

fn option_is_zero(arguments: &[String], name: &str) -> bool {
    arguments
        .iter()
        .any(|argument| argument == &format!("{name}=0"))
        || arguments
            .windows(2)
            .any(|window| window[0] == name && window[1] == "0")
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
