use crate::{MatchedRule, command::CommandInvocation};

use super::rule;

pub(super) fn match_invocation(invocation: &CommandInvocation) -> Option<MatchedRule> {
    if invocation.executable != "aws" {
        return None;
    }

    let arguments = &invocation.arguments;
    let (rule_id, reason_code, description, safer_action) =
        if has_sequence(arguments, &["secretsmanager", "delete-secret"]) {
            (
                "aws-secretsmanager-delete-secret",
                "aws_secrets.delete_secret",
                "Deletes a secret immediately or schedules it for deletion",
                "Export and inspect the secret, then use a reviewed recovery window",
            )
        } else if has_sequence(arguments, &["secretsmanager", "delete-resource-policy"]) {
            (
                "aws-secretsmanager-delete-resource-policy",
                "aws_secrets.delete_resource_policy",
                "Removes the resource policy and its cross-account access controls",
                "Export the policy and verify all consumers before removal",
            )
        } else if has_sequence(
            arguments,
            &["secretsmanager", "remove-regions-from-replication"],
        ) {
            (
                "aws-secretsmanager-remove-regions",
                "aws_secrets.remove_replica_regions",
                "Deletes secret replicas and reduces regional availability",
                "Review replica use and disaster recovery requirements first",
            )
        } else if has_sequence(arguments, &["secretsmanager", "update-secret"]) {
            (
                "aws-secretsmanager-update-secret",
                "aws_secrets.update_secret",
                "Changes a secret value, KMS key, or metadata",
                "Export the current version and coordinate the update with consumers",
            )
        } else if has_sequence(arguments, &["secretsmanager", "put-secret-value"]) {
            (
                "aws-secretsmanager-put-secret-value",
                "aws_secrets.put_secret_value",
                "Creates a new current secret version that can break consumers",
                "Use staging labels and coordinate a reviewed rotation",
            )
        } else if has_sequence(arguments, &["ssm", "delete-parameter"]) {
            (
                "aws-ssm-delete-parameter",
                "aws_secrets.delete_parameter",
                "Deletes an SSM parameter without a recovery window",
                "Export the parameter and verify every consumer before deletion",
            )
        } else if has_sequence(arguments, &["ssm", "delete-parameters"]) {
            (
                "aws-ssm-delete-parameters",
                "aws_secrets.delete_parameters",
                "Deletes multiple SSM parameters without a recovery window",
                "Export all values and remove one reviewed parameter at a time",
            )
        } else {
            return None;
        };

    Some(rule(
        "secrets.aws_secrets",
        rule_id,
        "command.arguments",
        reason_code,
        description,
        safer_action,
    ))
}

fn has_sequence(arguments: &[String], sequence: &[&str]) -> bool {
    arguments.windows(sequence.len()).any(|window| {
        window
            .iter()
            .map(String::as_str)
            .eq(sequence.iter().copied())
    })
}
