use crate::{MatchedRule, command::CommandInvocation};

use super::{git_operation, has_option, rule};

pub(super) fn match_invocation(invocation: &CommandInvocation) -> Option<MatchedRule> {
    if invocation.executable != "git" {
        return None;
    }
    let arguments = &invocation.arguments;
    let operation = git_operation(arguments)?;
    match operation {
        "checkout"
            if checkout_discards_paths(arguments)
                || has_option(arguments, "--force", Some('f')) =>
        {
            Some(git_rule(
                "checkout-discard",
                "git.checkout_discard",
                "Overwrites working-tree paths and discards uncommitted changes",
                "Stash changes or inspect git diff before restoring paths",
            ))
        }
        "checkout" if has_option(arguments, "-B", Some('B')) => Some(git_rule(
            "checkout-reset-branch",
            "git.checkout_reset_branch",
            "Resets an existing branch to a new starting point",
            "Create a new branch name or preserve the existing branch reference",
        )),
        "switch"
            if has_option(arguments, "--force", Some('f'))
                || has_option(arguments, "--discard-changes", None) =>
        {
            Some(git_rule(
                "switch-discard",
                "git.switch_discard",
                "Discards local changes while switching branches",
                "Stash changes and switch without discard options",
            ))
        }
        "switch"
            if has_option(arguments, "--force-create", Some('C'))
                || has_option(arguments, "-C", Some('C')) =>
        {
            Some(git_rule(
                "switch-reset-branch",
                "git.switch_reset_branch",
                "Resets an existing branch to a new starting point",
                "Create a new branch name or preserve the existing branch reference",
            ))
        }
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
        "push" if push_rewrites_or_prunes(arguments) => Some(git_rule(
            "push-rewrite",
            "git.push_rewrite",
            "Rewrites or removes remote references",
            "Use a normal push and review every remote reference change",
        )),
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
            "Review merged status and preserve the branch tip under a backup reference",
        )),
        "worktree" if worktree_discards_changes(arguments) => Some(git_rule(
            "worktree-destructive",
            "git.worktree_destructive",
            "Removes a dirty worktree or resets an existing branch",
            "Preserve worktree changes and use a new branch name",
        )),
        "submodule" if submodule_discards_changes(arguments) => Some(git_rule(
            "submodule-force",
            "git.submodule_force",
            "Discards local changes in a submodule worktree",
            "Preserve submodule changes and update without force",
        )),
        "rm" | "mv" | "checkout-index" if has_option(arguments, "--force", Some('f')) => {
            Some(git_rule(
                "force-path-change",
                "git.force_path_change",
                "Forcibly removes or overwrites tracked working-tree paths",
                "Preserve local changes and run the operation without force",
            ))
        }
        "read-tree"
            if has_option(arguments, "--reset", None)
                && has_option(arguments, "--update", Some('u')) =>
        {
            Some(git_rule(
                "read-tree-reset",
                "git.read_tree_reset",
                "Resets the index and working tree without protecting local changes",
                "Preserve local changes before updating the index",
            ))
        }
        "prune" => Some(git_rule(
            "prune-objects",
            "git.prune_objects",
            "Permanently removes unreachable Git objects",
            "Create a repository backup and inspect unreachable objects first",
        )),
        "gc" if has_option(arguments, "--prune", None) => Some(git_rule(
            "gc-prune",
            "git.gc_prune",
            "Expires unreachable objects and recovery history",
            "Use the default grace period after preserving required refs",
        )),
        "repack" if repack_expires_objects(arguments) => Some(git_rule(
            "repack-expire",
            "git.repack_expire",
            "Expires unreachable cruft objects during repacking",
            "Preserve required refs and omit immediate cruft expiration",
        )),
        "reflog" if reflog_removes_entries(arguments) => Some(git_rule(
            "reflog-remove",
            "git.reflog_remove",
            "Deletes reflog entries used to recover branch history",
            "Inspect and preserve recovery refs before expiring reflogs",
        )),
        "send-pack"
            if has_option(arguments, "--force", Some('f'))
                || has_option(arguments, "--mirror", None) =>
        {
            Some(git_rule(
                "send-pack-rewrite",
                "git.send_pack_rewrite",
                "Forcibly rewrites remote references",
                "Use a normal reviewed push",
            ))
        }
        "http-push"
            if has_option(arguments, "--force", Some('f'))
                || has_option(arguments, "--delete", Some('d'))
                || has_option(arguments, "-D", Some('D')) =>
        {
            Some(git_rule(
                "http-push-destructive",
                "git.http_push_destructive",
                "Forcibly rewrites or deletes remote references",
                "Use a normal reviewed push",
            ))
        }
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
    let trailing = &arguments[operation_index + 1..];
    has_option(trailing, "--pathspec-from-file", None)
        || trailing
            .windows(2)
            .any(|window| window[0] == "--" && !window[1].is_empty())
        || positional_after_options(trailing).len() >= 2
        || positional_after_options(trailing)
            .first()
            .is_some_and(|value| *value == "." || value.starts_with("./") || value.contains('/'))
}

fn restores_worktree(arguments: &[String]) -> bool {
    let staged_only = has_option(arguments, "--staged", Some('S'));
    !staged_only || has_option(arguments, "--worktree", Some('W'))
}

fn deletes_remote_ref(arguments: &[String]) -> bool {
    has_option(arguments, "--delete", Some('d'))
        || arguments.iter().any(|argument| {
            let refspec = argument.strip_prefix('+').unwrap_or(argument);
            refspec.len() > 1 && refspec.starts_with(':')
        })
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
fn push_rewrites_or_prunes(arguments: &[String]) -> bool {
    has_option(arguments, "--force", Some('f'))
        || has_option(arguments, "--force-with-lease", None)
        || has_option(arguments, "--mirror", None)
        || has_option(arguments, "--prune", None)
        || arguments.iter().any(|argument| {
            argument.starts_with('+') && !argument.starts_with("++") && argument.len() > 1
        })
}

fn worktree_discards_changes(arguments: &[String]) -> bool {
    has_sequence(arguments, &["worktree", "remove"]) && has_option(arguments, "--force", Some('f'))
        || has_sequence(arguments, &["worktree", "add"]) && has_option(arguments, "-B", Some('B'))
}

fn submodule_discards_changes(arguments: &[String]) -> bool {
    (has_sequence(arguments, &["submodule", "deinit"])
        || has_sequence(arguments, &["submodule", "update"]))
        && has_option(arguments, "--force", Some('f'))
}

fn repack_expires_objects(arguments: &[String]) -> bool {
    has_option(arguments, "--cruft", None)
        && has_option(arguments, "--cruft-expiration", None)
        && has_option(arguments, "--delete-redundant", Some('d'))
}

fn reflog_removes_entries(arguments: &[String]) -> bool {
    !has_option(arguments, "--dry-run", Some('n'))
        && arguments
            .iter()
            .any(|argument| matches!(argument.as_str(), "delete" | "drop" | "expire"))
}

fn has_sequence(arguments: &[String], sequence: &[&str]) -> bool {
    arguments.windows(sequence.len()).any(|window| {
        window
            .iter()
            .map(String::as_str)
            .eq(sequence.iter().copied())
    })
}

fn positional_after_options(arguments: &[String]) -> Vec<&str> {
    arguments
        .iter()
        .filter(|argument| !argument.starts_with('-'))
        .map(String::as_str)
        .collect()
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
