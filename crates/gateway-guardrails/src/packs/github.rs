use serde_json::Value;

use crate::{MatchedRule, selectors::McpCall};

use super::{first_string, normalize_identity, rule, server_has_identity};

pub(super) fn match_call(call: &McpCall) -> Option<MatchedRule> {
    if !server_has_identity(&call.server, &["github"]) {
        return None;
    }

    let tool = normalize_identity(&call.tool);
    match tool.as_str() {
        "delete_repository" => Some(github_rule(
            "delete-repository",
            "github.delete_repository",
            "Permanently deletes a GitHub repository",
            "Archive the repository and require an operator-approved deletion",
        )),
        "delete_file" => Some(github_rule(
            "delete-file",
            "github.delete_file",
            "Commits deletion of a repository file",
            "Preserve the file and review the target branch and path",
        )),
        "merge_pull_request" => Some(github_rule(
            "merge-pull-request",
            "github.merge_pull_request",
            "Merges changes into a protected base branch",
            "Review approvals, checks, and the exact merge method first",
        )),
        "delete_pending_pull_request_review" => Some(github_rule(
            "delete-pending-review",
            "github.delete_pending_review",
            "Deletes a pending pull request review",
            "Preserve or submit the review before deleting it",
        )),
        "remove_sub_issue" => Some(github_rule(
            "remove-sub-issue",
            "github.remove_sub_issue",
            "Removes a sub-issue relationship",
            "Review the parent-child relationship before removal",
        )),
        "create_or_update_file" if has_nonempty_string(&call.arguments, "sha") => {
            Some(github_rule(
                "overwrite-file",
                "github.overwrite_file",
                "Replaces existing repository file content",
                "Read the current file and review the replacement commit first",
            ))
        }
        "actions_run_trigger"
            if method_is(
                &call.arguments,
                &["cancel_workflow_run", "delete_workflow_run_logs"],
            ) =>
        {
            Some(github_rule(
                "destructive-workflow-action",
                "github.destructive_workflow_action",
                "Cancels a workflow run or deletes its logs",
                "Inspect the run and preserve required logs before changing it",
            ))
        }
        "cancel_workflow_run" | "delete_workflow_run_logs" => Some(github_rule(
            "destructive-workflow-action",
            "github.destructive_workflow_action",
            "Cancels a workflow run or deletes its logs",
            "Inspect the run and preserve required logs before changing it",
        )),
        "update_pull_request" | "update_pull_request_state"
            if string_is(&call.arguments, "state", "closed") =>
        {
            Some(github_rule(
                "close-pull-request",
                "github.close_pull_request",
                "Closes a pull request",
                "Confirm the pull request should not be merged before closing it",
            ))
        }
        "pull_request_review_write" if method_is(&call.arguments, &["delete_pending"]) => {
            Some(github_rule(
                "delete-pending-review",
                "github.delete_pending_review",
                "Deletes a pending pull request review",
                "Preserve or submit the review before deleting it",
            ))
        }
        "projects_write"
            if method_is(
                &call.arguments,
                &["delete_project_item", "delete_project_view"],
            ) =>
        {
            Some(github_rule(
                "delete-project-state",
                "github.delete_project_state",
                "Deletes a GitHub project item or view",
                "Export the project state and review the exact target first",
            ))
        }
        "delete_project_item" => Some(github_rule(
            "delete-project-state",
            "github.delete_project_state",
            "Deletes a GitHub project item",
            "Export the project state and review the exact target first",
        )),
        "label_write" if method_is(&call.arguments, &["delete"]) => Some(github_rule(
            "delete-label",
            "github.delete_label",
            "Deletes a repository label",
            "Review every issue and pull request that uses the label",
        )),
        "discussion_comment_write" if method_is(&call.arguments, &["delete"]) => Some(github_rule(
            "delete-discussion-comment",
            "github.delete_discussion_comment",
            "Deletes a GitHub discussion comment",
            "Preserve the comment and verify the exact discussion first",
        )),
        "discussion_comment_write" if method_is(&call.arguments, &["update"]) => Some(github_rule(
            "overwrite-discussion-comment",
            "github.overwrite_discussion_comment",
            "Replaces GitHub discussion comment content",
            "Read and preserve the current comment before replacing it",
        )),
        "issue_write"
            if method_is(&call.arguments, &["update"])
                && (string_is(&call.arguments, "state", "closed")
                    || array_has_delete(&call.arguments, "issue_fields")) =>
        {
            Some(github_rule(
                "remove-issue-state",
                "github.remove_issue_state",
                "Closes an issue or deletes one of its project field values",
                "Review the issue and preserve its current project metadata",
            ))
        }
        "update_issue_state" if string_is(&call.arguments, "state", "closed") => Some(github_rule(
            "close-issue",
            "github.close_issue",
            "Closes a GitHub issue",
            "Confirm the issue is complete or obsolete before closing it",
        )),
        "sub_issue_write" | "issue_dependency_write" if method_is(&call.arguments, &["remove"]) => {
            Some(github_rule(
                "remove-issue-relationship",
                "github.remove_issue_relationship",
                "Removes a GitHub issue relationship",
                "Review the dependency or parent-child relationship first",
            ))
        }
        "set_issue_fields" if array_has_delete(&call.arguments, "fields") => Some(github_rule(
            "delete-issue-field",
            "github.delete_issue_field",
            "Deletes a project field value from an issue",
            "Preserve the current value and review the target field first",
        )),
        _ => None,
    }
}

fn method_is(arguments: &Value, expected: &[&str]) -> bool {
    first_string(arguments, "$.method")
        .map(normalize_identity)
        .is_some_and(|method| expected.contains(&method.as_str()))
}

fn string_is(arguments: &Value, field: &str, expected: &str) -> bool {
    arguments
        .get(field)
        .and_then(Value::as_str)
        .is_some_and(|value| value.eq_ignore_ascii_case(expected))
}

fn has_nonempty_string(arguments: &Value, field: &str) -> bool {
    arguments
        .get(field)
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
}

fn array_has_delete(arguments: &Value, field: &str) -> bool {
    arguments
        .get(field)
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items
                .iter()
                .any(|item| item.get("delete").and_then(Value::as_bool) == Some(true))
        })
}

fn github_rule(
    rule_id: &str,
    reason_code: &str,
    description: &str,
    safer_action: &str,
) -> MatchedRule {
    rule(
        "saas.github",
        rule_id,
        "tool",
        reason_code,
        description,
        safer_action,
    )
}
