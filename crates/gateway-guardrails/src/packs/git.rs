use crate::{MatchedRule, command::CommandInvocation};

use super::{git_operation, has_option, rule};

pub(super) fn match_invocation(invocation: &CommandInvocation) -> Option<MatchedRule> {
    if invocation.executable != "git" {
        return None;
    }
    let arguments = &invocation.arguments;
    let operation = git_operation(arguments)?;
    match operation {
        "checkout" if checkout_discards_paths(arguments) => Some(git_rule(
            "checkout-discard",
            "git.checkout_discard",
            "Overwrites working-tree paths and discards uncommitted changes",
            "Stash changes or inspect git diff before restoring paths",
        )),
        "restore" if restores_worktree(arguments) => Some(git_rule(
            "restore-worktree",
            "git.restore_worktree",
            "Discards uncommitted working-tree changes",
            "Inspect git diff and stash changes before restoring paths",
        )),
        "reset" if has_option(arguments, "--hard", None) => Some(git_rule(
            "reset-hard",
            "git.reset_hard",
            "Discards tracked working-tree changes",
            "Create a backup branch or use git stash before resetting",
        )),
        "reset" if has_option(arguments, "--merge", None) => Some(git_rule(
            "reset-merge",
            "git.reset_merge",
            "Can discard uncommitted changes while resetting the index",
            "Stash changes and inspect the target commit before resetting",
        )),
        "clean" if has_option(arguments, "--force", Some('f')) => Some(git_rule(
            "clean-force",
            "git.clean_force",
            "Deletes untracked files",
            "Run git clean --dry-run and remove reviewed paths explicitly",
        )),
        "push"
            if has_option(arguments, "--force", Some('f'))
                || has_option(arguments, "--force-with-lease", None) =>
        {
            Some(git_rule(
                "push-force",
                "git.push_force",
                "Rewrites remote branch history",
                "Use a normal push or coordinate a reviewed force-with-lease operation",
            ))
        }
        "push" if deletes_remote_ref(arguments) => Some(git_rule(
            "push-delete",
            "git.push_delete",
            "Deletes a remote reference",
            "Delete the remote reference through a reviewed repository workflow",
        )),
        "branch" if forces_or_deletes_branch(arguments) => Some(git_rule(
            "branch-force-delete",
            "git.branch_force_delete",
            "Deletes a branch or forcibly updates a branch reference",
            "Use git branch -d after merge, or create a new branch name",
        )),
        "stash" if arguments.iter().any(|argument| argument == "drop") => Some(git_rule(
            "stash-drop",
            "git.stash_drop",
            "Deletes a saved stash entry",
            "Inspect git stash show and apply the stash before deleting it",
        )),
        "stash" if arguments.iter().any(|argument| argument == "clear") => Some(git_rule(
            "stash-clear",
            "git.stash_clear",
            "Deletes every saved stash entry",
            "List and preserve required stashes before removing any entry",
        )),
        _ => None,
    }
}

fn checkout_discards_paths(arguments: &[String]) -> bool {
    let Some(operation_index) = arguments.iter().position(|argument| argument == "checkout") else {
        return false;
    };
    arguments[operation_index + 1..]
        .windows(2)
        .any(|window| window[0] == "--" && !window[1].is_empty())
}

fn restores_worktree(arguments: &[String]) -> bool {
    let staged_only = has_option(arguments, "--staged", Some('S'));
    !staged_only || has_option(arguments, "--worktree", Some('W'))
}

fn deletes_remote_ref(arguments: &[String]) -> bool {
    has_option(arguments, "--delete", Some('d'))
        || arguments
            .iter()
            .any(|argument| argument.len() > 1 && argument.starts_with(':'))
}

fn forces_or_deletes_branch(arguments: &[String]) -> bool {
    arguments.iter().any(|argument| {
        argument == "--delete"
            || argument == "--force"
            || argument.strip_prefix('-').is_some_and(|flags| {
                !flags.starts_with('-')
                    && flags
                        .chars()
                        .any(|flag| matches!(flag, 'd' | 'D' | 'f' | 'M' | 'C'))
            })
    })
}

fn git_rule(
    rule_id: &str,
    reason_code: &str,
    description: &str,
    safer_action: &str,
) -> MatchedRule {
    rule(
        "core.git",
        rule_id,
        "command.arguments",
        reason_code,
        description,
        safer_action,
    )
}
