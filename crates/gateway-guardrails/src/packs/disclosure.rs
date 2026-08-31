use crate::{MatchedRule, command::CommandInvocation};

use super::{has_option, rule};

pub(super) fn match_invocation(invocation: &CommandInvocation) -> Option<MatchedRule> {
    let arguments = &invocation.arguments;
    if arguments
        .iter()
        .any(|argument| matches!(argument.as_str(), "-h" | "--help" | "help"))
    {
        return None;
    }
    let (rule_id, description, safer_action) = match invocation.executable.as_str() {
        "infisical" if has_sequence(arguments, &["secrets", "get"]) => (
            "infisical-secrets-get-output",
            "infisical secrets get prints secret values to agent-visible output",
            "Use infisical run to inject values into the consuming process",
        ),
        "infisical" if arguments.iter().any(|argument| argument == "export") => (
            "infisical-export-output",
            "infisical export emits a complete set of secret values",
            "Use infisical run instead of exporting values",
        ),
        "infisical" if has_sequence(arguments, &["dynamic-secrets", "lease", "create"]) => (
            "infisical-dynamic-lease-create-output",
            "Creating a dynamic secret lease emits newly issued credentials",
            "Create the lease through a protected workflow outside the agent transcript",
        ),
        "infisical"
            if arguments.iter().any(|argument| argument == "secrets")
                && !arguments
                    .iter()
                    .any(|argument| matches!(argument.as_str(), "set" | "delete" | "folders")) =>
        {
            (
                "infisical-secrets-list-output",
                "infisical secrets prints selected secret values to agent-visible output",
                "Use infisical run to inject values into the consuming process",
            )
        }
        "op" if arguments.iter().any(|argument| argument == "read") => (
            "onepassword-read-output",
            "op read prints a 1Password field value to agent-visible output",
            "Use op run or a secret reference consumed directly by the target process",
        ),
        "op" if has_sequence(arguments, &["item", "get"]) => (
            "onepassword-item-get-output",
            "op item get can print secret fields to agent-visible output",
            "Use op run for value injection or request only non-secret metadata",
        ),
        "op" if has_sequence(arguments, &["document", "get"]) => (
            "onepassword-document-get-output",
            "op document get emits protected document contents",
            "Retrieve the document through a protected workflow outside the agent transcript",
        ),
        "op" if arguments.iter().any(|argument| argument == "inject") => (
            "onepassword-inject-output",
            "op inject emits substituted secret values",
            "Use op run to inject values directly into the consuming process",
        ),
        "op" if has_sequence(arguments, &["environment", "read"]) => (
            "onepassword-environment-read-output",
            "op environment read prints protected environment values",
            "Inject values directly into the consuming process",
        ),
        "op" if has_sequence(arguments, &["connect", "token", "create"])
            || has_sequence(arguments, &["service-account", "create"]) =>
        {
            (
                "onepassword-token-create-output",
                "Creating a 1Password token emits the credential once",
                "Create and deliver the token through a protected operator workflow",
            )
        }
        "op" if (arguments.iter().any(|argument| argument == "signin")
            || has_sequence(arguments, &["account", "add"]))
            && has_option(arguments, "--raw", None) =>
        {
            (
                "onepassword-session-token-output",
                "The raw option prints a 1Password session token",
                "Use biometric or desktop-app integration without printing the token",
            )
        }
        "doppler"
            if has_sequence(arguments, &["secrets", "substitute"])
                || (arguments.iter().any(|argument| argument == "secrets")
                    && !arguments.iter().any(|argument| {
                        matches!(
                            argument.as_str(),
                            "set" | "delete" | "upload" | "--only-names"
                        )
                    })) =>
        {
            (
                "doppler-secrets-output",
                "doppler secrets emits secret values to agent-visible output",
                "Use doppler run to inject values into the consuming process",
            )
        }
        "vault"
            if arguments.iter().any(|argument| argument == "read")
                || has_sequence(arguments, &["kv", "get"]) =>
        {
            (
                "vault-read-output",
                "Vault read operations can print secret values to agent-visible output",
                "Deliver values directly to the consuming process through a protected workflow",
            )
        }
        "vault"
            if arguments.iter().any(|argument| argument == "login")
                || has_sequence(arguments, &["token", "create"])
                || has_sequence(arguments, &["operator", "init"])
                || has_sequence(arguments, &["operator", "rekey"])
                || has_sequence(arguments, &["operator", "generate-root"]) =>
        {
            (
                "vault-credential-output",
                "Vault authentication and operator commands emit tokens or recovery keys",
                "Run the command through a protected operator workflow",
            )
        }
        "aws"
            if has_sequence(arguments, &["secretsmanager", "get-secret-value"])
                || has_sequence(arguments, &["secretsmanager", "batch-get-secret-value"]) =>
        {
            (
                "aws-secretsmanager-read-output",
                "AWS Secrets Manager read operations print stored secret values",
                "Inject the value into the intended process without printing it",
            )
        }
        "aws" if has_sequence(arguments, &["secretsmanager", "get-random-password"]) => (
            "aws-random-password-output",
            "AWS Secrets Manager prints a newly generated credential",
            "Create and deliver the credential through a protected workflow",
        ),
        "aws"
            if arguments.iter().any(|argument| {
                matches!(
                    argument.as_str(),
                    "get-parameter"
                        | "get-parameters"
                        | "get-parameters-by-path"
                        | "get-parameter-history"
                )
            }) && has_option(arguments, "--with-decryption", None) =>
        {
            (
                "aws-ssm-decrypted-read-output",
                "Decrypted SSM parameter reads print SecureString values",
                "Pass decrypted values directly to the consuming process",
            )
        }
        _ => return None,
    };

    Some(rule(
        "secret_disclosure",
        rule_id,
        "command.arguments",
        &format!("secret_disclosure.{rule_id}"),
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
