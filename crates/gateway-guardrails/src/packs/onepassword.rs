use crate::{MatchedRule, command::CommandInvocation};

use super::rule;

pub(super) fn match_invocation(invocation: &CommandInvocation) -> Option<MatchedRule> {
    if invocation.executable != "op" {
        return None;
    }

    let (rule_id, reason_code, description, safer_action) =
        if has_sequence(&invocation.arguments, &["item", "delete"]) {
            (
                "op-item-delete",
                "onepassword.item_delete",
                "Deletes or archives a 1Password secret item",
                "Export required fields and prefer an archive operation with a recovery plan",
            )
        } else if has_sequence(&invocation.arguments, &["document", "delete"]) {
            (
                "op-document-delete",
                "onepassword.document_delete",
                "Deletes or archives a protected 1Password document",
                "Download a protected backup before removing the document",
            )
        } else if has_sequence(&invocation.arguments, &["vault", "delete"]) {
            (
                "op-vault-delete",
                "onepassword.vault_delete",
                "Deletes a vault and all items, documents, and permissions in it",
                "Inventory and export the vault before a reviewed deletion",
            )
        } else if has_sequence(&invocation.arguments, &["user", "delete"]) {
            (
                "op-user-delete",
                "onepassword.user_delete",
                "Removes a user and revokes their vault access",
                "Suspend the user and transfer owned resources before deletion",
            )
        } else if has_sequence(&invocation.arguments, &["group", "delete"]) {
            (
                "op-group-delete",
                "onepassword.group_delete",
                "Deletes a group and its vault permission assignments",
                "Review membership and replace required permission assignments first",
            )
        } else if has_sequence(&invocation.arguments, &["connect", "token", "delete"]) {
            (
                "op-connect-token-delete",
                "onepassword.connect_token_delete",
                "Revokes a 1Password Connect access token",
                "Rotate consumers to a replacement token before revocation",
            )
        } else if has_sequence(&invocation.arguments, &["item", "edit"])
            || has_sequence(&invocation.arguments, &["document", "edit"])
        {
            (
                "op-content-edit",
                "onepassword.content_edit",
                "Overwrites protected item fields or document contents",
                "Preserve the current item and review each changed field",
            )
        } else if has_sequence(&invocation.arguments, &["item", "move"]) {
            (
                "op-item-move",
                "onepassword.item_move",
                "Deletes the original item after copying it to another vault",
                "Verify destination access and preserve the original item first",
            )
        } else if has_sequence(&invocation.arguments, &["user", "suspend"]) {
            (
                "op-user-suspend",
                "onepassword.user_suspend",
                "Immediately revokes a user's access to 1Password data",
                "Transfer required ownership and use a reviewed access change",
            )
        } else if has_sequence(&invocation.arguments, &["group", "user", "revoke"])
            || has_sequence(&invocation.arguments, &["vault", "group", "revoke"])
            || has_sequence(&invocation.arguments, &["vault", "user", "revoke"])
            || has_sequence(&invocation.arguments, &["connect", "group", "revoke"])
            || has_sequence(&invocation.arguments, &["connect", "vault", "revoke"])
        {
            (
                "op-access-revoke",
                "onepassword.access_revoke",
                "Revokes group, user, or Connect access to protected vault data",
                "Review affected consumers and stage a replacement access path",
            )
        } else if has_sequence(&invocation.arguments, &["connect", "server", "delete"]) {
            (
                "op-connect-server-delete",
                "onepassword.connect_server_delete",
                "Deletes a Connect server and invalidates its access",
                "Migrate every consumer before deleting the Connect server",
            )
        } else {
            return None;
        };

    Some(rule(
        "secrets.onepassword",
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
