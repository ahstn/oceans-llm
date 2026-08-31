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
