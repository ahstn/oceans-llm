use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

mod aws_secrets;
mod disclosure;
mod git;
mod github;
mod helm;
mod kubectl;
mod onepassword;
mod snowflake;

use crate::{
    DeterministicEvaluator, EffectivePolicy, EvaluationError, EvaluationInput, EvaluationPayload,
    MatchedRule, ReasonCode,
    command::{CommandInvocation, has_pipeline_to, has_truncating_redirection, parse_command_line},
    selectors::{JsonPath, McpCall},
};

pub const BUILT_IN_PACK_IDS: [&str; 14] = [
    "core.shell",
    "core.git",
    "core.filesystem",
    "database.postgresql",
    "database.snowflake",
    "cloud.aws",
    "cloud.gcp",
    "kubernetes.kubectl",
    "kubernetes.helm",
    "secrets.aws_secrets",
    "secrets.onepassword",
    "secret_disclosure",
    "saas.github",
    "saas.notion",
];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PackId(String);

impl PackId {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidPackId> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 64
            || !value.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'.' | b'-' | b'_')
            })
        {
            return Err(InvalidPackId(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PackId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for PackId {
    type Err = InvalidPackId;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl Serialize for PackId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for PackId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid guardrail pack ID `{0}`")]
pub struct InvalidPackId(String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PackMetadata {
    pub id: PackId,
    pub version: &'static str,
    pub description: &'static str,
}

pub struct PackRegistry;

impl PackRegistry {
    pub fn built_in() -> Vec<PackMetadata> {
        vec![
            metadata("core.shell", "Destructive operating-system operations"),
            metadata("core.git", "Destructive Git repository operations"),
            metadata("core.filesystem", "Destructive filesystem operations"),
            metadata("database.postgresql", "Destructive PostgreSQL operations"),
            metadata(
                "database.snowflake",
                "Destructive Snowflake CLI and SQL operations",
            ),
            metadata("cloud.aws", "Destructive AWS CLI and MCP operations"),
            metadata(
                "cloud.gcp",
                "Destructive Google Cloud CLI and MCP operations",
            ),
            metadata("kubernetes.kubectl", "Destructive kubectl operations"),
            metadata("kubernetes.helm", "Destructive Helm operations"),
            metadata(
                "secrets.aws_secrets",
                "Destructive AWS Secrets Manager and SSM operations",
            ),
            metadata(
                "secrets.onepassword",
                "Destructive 1Password CLI operations",
            ),
            metadata(
                "secret_disclosure",
                "Secret-manager commands that disclose credential values",
            ),
            metadata("saas.github", "Destructive GitHub MCP operations"),
            metadata("saas.notion", "Destructive Notion workspace operations"),
        ]
    }

    pub fn contains(id: &str) -> bool {
        BUILT_IN_PACK_IDS.contains(&id)
    }
}

fn metadata(id: &str, description: &'static str) -> PackMetadata {
    PackMetadata {
        id: PackId::new(id).expect("static pack ID is valid"),
        version: "1.0.0",
        description,
    }
}

#[derive(Debug, Default)]
pub struct BuiltInEvaluator;

impl DeterministicEvaluator for BuiltInEvaluator {
    fn id(&self) -> &str {
        "built_in"
    }

    fn evaluate(
        &self,
        input: &EvaluationInput,
        policy: &EffectivePolicy,
    ) -> Result<Option<MatchedRule>, EvaluationError> {
        if matches!(
            &input.payload,
            EvaluationPayload::ToolCall { arguments, .. } if !arguments.is_object()
        ) {
            return Err(EvaluationError::MalformedToolCall);
        }

        for pack in &policy.packs {
            let matched = match (&input.payload, pack.as_str()) {
                (EvaluationPayload::ShellCommand { command }, pack) => {
                    match_shell_pack(pack, command)
                }
                (EvaluationPayload::ToolCall { name, arguments }, pack) => {
                    match_tool_call_pack(pack, name, arguments)
                }
                (EvaluationPayload::McpCall { call }, pack) => match_mcp_pack(pack, call),
                _ => None,
            };
            if matched.is_some() {
                return Ok(matched);
            }
        }
        Ok(None)
    }
}

fn match_shell_tool(pack: &str, name: &str, arguments: &Value) -> Option<MatchedRule> {
    if !is_shell_tool(name) {
        return None;
    }
    for path in [
        "$.command",
        "$.cmd",
        "$.input.command",
        "$.arguments.command",
    ] {
        if let Some(command) = first_string(arguments, path)
            && let Some(matched) = match_shell_pack(pack, command)
        {
            return Some(MatchedRule {
                matched_field: path.to_string(),
                ..matched
            });
        }
    }
    None
}

fn match_tool_call_pack(pack: &str, name: &str, arguments: &Value) -> Option<MatchedRule> {
    if let Some(matched) = match_shell_tool(pack, name, arguments) {
        return Some(matched);
    }

    let call = McpCall {
        server: "generated_tool_call".to_string(),
        tool: name.to_string(),
        arguments: arguments.clone(),
    };
    match_mcp_pack(pack, &call)
}

fn is_shell_tool(name: &str) -> bool {
    matches!(
        normalize_identity(name).as_str(),
        "shell" | "bash" | "terminal" | "run_command" | "execute_command" | "computer.run_command"
    )
}

fn match_shell_pack(pack: &str, command: &str) -> Option<MatchedRule> {
    if pack == "core.filesystem" && has_truncating_redirection(command) {
        return Some(rule(
            "core.filesystem",
            "truncate-redirection",
            "command.redirection",
            "filesystem.truncate_redirection",
            "Truncates or replaces the contents of a redirected output file",
            "Write to a new reviewed path or preserve the existing file first",
        ));
    }
    if pack == "database.postgresql" && has_pipeline_to(command, &["psql", "pgcli"]) {
        return Some(rule(
            "database.postgresql",
            "uninspectable-sql-input",
            "command.pipeline",
            "postgresql.uninspectable_sql_input",
            "Pipes PostgreSQL statements from input that guardrails cannot inspect",
            "Provide the reviewed SQL through --command",
        ));
    }
    let invocations = parse_command_line(command);
    for invocation in &invocations {
        let finding = match pack {
            "core.shell" => match_core_shell(invocation),
            "core.git" => git::match_invocation(invocation),
            "core.filesystem" => match_filesystem(invocation),
            "database.postgresql" => match_postgresql_invocation(invocation),
            "database.snowflake" => snowflake::match_invocation(invocation),
            "cloud.aws" => match_aws(invocation),
            "cloud.gcp" => match_gcp(invocation),
            "kubernetes.kubectl" => kubectl::match_invocation(invocation),
            "kubernetes.helm" => helm::match_invocation(invocation),
            "secrets.aws_secrets" => aws_secrets::match_invocation(invocation),
            "secrets.onepassword" => onepassword::match_invocation(invocation),
            "secret_disclosure" => disclosure::match_invocation(invocation),
            "saas.notion" => match_notion_cli(invocation),
            _ => None,
        };
        if finding.is_some() {
            return finding;
        }
    }
    None
}

fn match_core_shell(invocation: &CommandInvocation) -> Option<MatchedRule> {
    match invocation.executable.as_str() {
        "shutdown" | "reboot" | "poweroff" | "halt" => Some(rule(
            "core.shell",
            "system-power-state",
            "command.executable",
            "shell.system_power_state",
            "Changes the host power state",
            "Use an operator-approved maintenance procedure",
        )),
        "systemctl" if systemctl_changes_power_state(&invocation.arguments) => Some(rule(
            "core.shell",
            "systemctl-power-state",
            "command.arguments",
            "shell.systemctl_power_state",
            "Changes the host power state through systemd",
            "Use an operator-approved maintenance procedure",
        )),
        "init" | "telinit" if targets_shutdown_runlevel(&invocation.arguments) => Some(rule(
            "core.shell",
            "init-power-state",
            "command.arguments",
            "shell.init_power_state",
            "Changes the host power state through an init runlevel",
            "Use an operator-approved maintenance procedure",
        )),
        executable if executable.starts_with("mkfs") => Some(rule(
            "core.shell",
            "format-filesystem",
            "command.executable",
            "shell.format_filesystem",
            "Formats a block device",
            "Verify the target device and use a provisioned replacement volume",
        )),
        "kill" if targets_pid_one(&invocation.arguments) => Some(rule(
            "core.shell",
            "kill-init",
            "command.arguments",
            "shell.kill_init",
            "Terminates the host init process",
            "Restart the intended service through its service manager",
        )),
        "kill" if targets_broadcast_processes(&invocation.arguments) => Some(rule(
            "core.shell",
            "kill-broadcast",
            "command.arguments",
            "shell.kill_broadcast",
            "Sends a signal to every permitted process or a process group",
            "Target one reviewed process ID through its service manager",
        )),
        executable
            if matches!(executable, "sh" | "bash" | "zsh" | "dash" | "fish")
                && crate::command::nested_shell_command(invocation).is_none() =>
        {
            Some(rule(
                "core.shell",
                "uninspectable-shell-input",
                "command.arguments",
                "shell.uninspectable_input",
                "Runs shell input that the policy cannot inspect",
                "Pass the command through the shell command option",
            ))
        }
        executable if executable.starts_with('$') => Some(rule(
            "core.shell",
            "dynamic-executable",
            "command.executable",
            "shell.dynamic_executable",
            "Resolves the executable from an unknown shell variable",
            "Use an explicit executable name that policy can inspect",
        )),
        _ => None,
    }
}

fn systemctl_changes_power_state(arguments: &[String]) -> bool {
    let positional = positional_arguments(arguments);
    positional.iter().any(|argument| {
        matches!(
            argument.as_str(),
            "poweroff" | "reboot" | "halt" | "kexec" | "soft-reboot"
        )
    }) || positional.windows(2).any(|window| {
        window[0] == "start"
            && matches!(
                window[1].as_str(),
                "poweroff.target" | "reboot.target" | "halt.target"
            )
    })
}

fn targets_shutdown_runlevel(arguments: &[String]) -> bool {
    arguments
        .iter()
        .any(|argument| matches!(argument.as_str(), "0" | "6"))
}

fn targets_pid_one(arguments: &[String]) -> bool {
    arguments
        .iter()
        .filter(|argument| !argument.starts_with('-'))
        .any(|argument| argument == "1")
}

fn targets_broadcast_processes(arguments: &[String]) -> bool {
    let mut index = 0;
    while index < arguments.len() {
        let argument = arguments[index].as_str();
        if argument == "--" {
            return arguments[index + 1..].iter().any(|target| target == "-1");
        }
        if matches!(argument, "-s" | "--signal" | "-n") {
            index += 2;
            continue;
        }
        if argument.starts_with("--signal=") {
            index += 1;
            continue;
        }
        if index == 0 && argument.starts_with('-') {
            index += 1;
            continue;
        }
        if argument == "-1" {
            return true;
        }
        index += 1;
    }
    false
}

fn match_filesystem(invocation: &CommandInvocation) -> Option<MatchedRule> {
    let arguments = &invocation.arguments;
    match invocation.executable.as_str() {
        "rm" if (has_option(arguments, "--recursive", Some('r'))
            || has_option(arguments, "--recursive", Some('R')))
            && has_option(arguments, "--force", Some('f')) =>
        {
            Some(rule(
                "core.filesystem",
                "recursive-force-remove",
                "command.arguments",
                "filesystem.recursive_force_remove",
                "Recursively deletes files without confirmation",
                "List and review the target, then remove explicit paths without force",
            ))
        }
        "rm" if has_option(arguments, "--recursive", Some('r'))
            || has_option(arguments, "--recursive", Some('R')) =>
        {
            Some(rule(
                "core.filesystem",
                "recursive-remove",
                "command.arguments",
                "filesystem.recursive_remove",
                "Recursively deletes a directory tree",
                "List and review the target, then remove explicit paths",
            ))
        }
        "rmdir" | "unlink" => Some(rule(
            "core.filesystem",
            "remove-path",
            "command.executable",
            "filesystem.remove_path",
            "Removes a filesystem path",
            "Verify and preserve the target before removing it",
        )),
        "shred" => Some(rule(
            "core.filesystem",
            "shred-file",
            "command.executable",
            "filesystem.shred_file",
            "Overwrites file contents to prevent recovery",
            "Preserve required data and use a reviewed disposal procedure",
        )),
        "truncate" if truncate_shrinks(arguments) => Some(rule(
            "core.filesystem",
            "truncate-file",
            "command.arguments",
            "filesystem.truncate_file",
            "Shrinks or clears file contents",
            "Preserve the file and write the intended content to a new path",
        )),
        "dd" if arguments
            .iter()
            .any(|argument| argument.starts_with("of=/dev/")) =>
        {
            Some(rule(
                "core.filesystem",
                "overwrite-device",
                "command.arguments",
                "filesystem.overwrite_device",
                "Writes raw data to a block or character device",
                "Verify the device and use an operator-approved imaging procedure",
            ))
        }
        "wipefs"
            if has_option(arguments, "--all", Some('a'))
                || has_option(arguments, "--offset", Some('o')) =>
        {
            Some(rule(
                "core.filesystem",
                "wipe-filesystem-signature",
                "command.arguments",
                "filesystem.wipe_signature",
                "Erases filesystem, RAID, or partition-table signatures",
                "Inventory the device and preserve metadata before erasing signatures",
            ))
        }
        "diskutil" if diskutil_erases_storage(arguments) => Some(rule(
            "core.filesystem",
            "diskutil-erase",
            "command.arguments",
            "filesystem.diskutil_erase",
            "Erases, repartitions, or deletes a macOS storage volume",
            "Verify the disk identifier and preserve required data first",
        )),
        "find" if arguments.iter().any(|argument| argument == "-delete") => Some(rule(
            "core.filesystem",
            "find-delete",
            "command.arguments",
            "filesystem.find_delete",
            "Deletes every path selected by find",
            "Run find without -delete and review the selected paths",
        )),
        _ => None,
    }
}

fn truncate_shrinks(arguments: &[String]) -> bool {
    option_value(arguments, &["-s", "--size"]).is_some_and(|size| {
        let (negative, magnitude) = size
            .strip_prefix('-')
            .map_or((false, size), |value| (true, value));
        let digit_count = magnitude.bytes().take_while(u8::is_ascii_digit).count();
        if digit_count == 0
            || !magnitude[digit_count..]
                .bytes()
                .all(|byte| byte.is_ascii_alphabetic())
        {
            return false;
        }
        negative || magnitude[..digit_count].bytes().all(|byte| byte == b'0')
    })
}

fn diskutil_erases_storage(arguments: &[String]) -> bool {
    arguments.iter().any(|argument| {
        matches!(
            argument.to_ascii_lowercase().as_str(),
            "erasedisk" | "erasevolume" | "partitiondisk" | "deletevolume"
        )
    })
}

fn match_postgresql_invocation(invocation: &CommandInvocation) -> Option<MatchedRule> {
    if !matches!(invocation.executable.as_str(), "psql" | "pgcli") {
        return None;
    }
    if option_value(&invocation.arguments, &["-f", "--file"]).is_some()
        || invocation.arguments.iter().any(|argument| argument == "-")
    {
        return Some(rule(
            "database.postgresql",
            "uninspectable-sql-input",
            "command.arguments",
            "postgresql.uninspectable_sql_input",
            "Loads PostgreSQL statements from input that guardrails cannot inspect",
            "Provide the reviewed SQL through --command",
        ));
    }
    option_value(&invocation.arguments, &["-c", "--command"])
        .and_then(|sql| match_destructive_sql(sql, "command.arguments"))
}

fn match_destructive_sql(sql: &str, field: &str) -> Option<MatchedRule> {
    sql_statements(sql)
        .into_iter()
        .find_map(|statement| match_destructive_sql_statement(&statement, field))
}

fn match_destructive_sql_statement(sql: &str, field: &str) -> Option<MatchedRule> {
    let words = top_level_sql_words(sql);
    let first = words.first()?;
    let operation_index = if is_destructive_sql_operation(first) {
        0
    } else if matches!(first.as_str(), "with" | "explain") {
        words
            .iter()
            .position(|word| is_destructive_sql_operation(word))?
    } else {
        return None;
    };
    let normalized = &words[operation_index..];
    let first = normalized.first()?;
    let second = normalized.get(1).map(String::as_str);
    match (first.as_str(), second) {
        ("drop", Some("database")) => Some(rule(
            "database.postgresql",
            "drop-database",
            field,
            "postgresql.drop_database",
            "Drops a PostgreSQL database",
            "Take a verified backup and use a reviewed migration",
        )),
        ("drop", Some("schema")) => Some(rule(
            "database.postgresql",
            "drop-schema",
            field,
            "postgresql.drop_schema",
            "Drops a PostgreSQL schema",
            "Use a reviewed migration after a verified backup",
        )),
        ("drop", Some("table")) => Some(rule(
            "database.postgresql",
            "drop-table",
            field,
            "postgresql.drop_table",
            "Drops a PostgreSQL table",
            "Use a reviewed migration after a verified backup",
        )),
        ("drop", Some(object)) => Some(rule(
            "database.postgresql",
            "drop-object",
            field,
            "postgresql.drop_object",
            &format!("Drops a PostgreSQL {object} object"),
            "Preserve the object definition and use a reviewed migration",
        )),
        ("truncate", _) => Some(rule(
            "database.postgresql",
            "truncate",
            field,
            "postgresql.truncate",
            "Removes all rows from a table",
            "Use a scoped DELETE in a transaction after a verified backup",
        )),
        ("delete", Some("from")) if !normalized.iter().any(|word| word == "where") => Some(rule(
            "database.postgresql",
            "delete-without-where",
            field,
            "postgresql.delete_without_where",
            "Deletes every row from a table",
            "Add a reviewed WHERE clause and run inside a transaction",
        )),
        ("alter", Some("table")) if postgres_alter_removes_state(normalized) => Some(rule(
            "database.postgresql",
            "alter-table-remove",
            field,
            "postgresql.alter_table_remove",
            "Drops a table column or constraint, or detaches a partition",
            "Preserve the table definition and use a reviewed migration",
        )),
        _ => None,
    }
}

fn is_destructive_sql_operation(word: &str) -> bool {
    matches!(word, "drop" | "truncate" | "delete" | "alter")
}

fn postgres_alter_removes_state(words: &[String]) -> bool {
    words.windows(2).any(|window| {
        matches!(
            window,
            [operation, object]
                if (operation == "drop"
                    && matches!(object.as_str(), "column" | "constraint"))
                    || (operation == "detach" && object == "partition")
        )
    })
}

fn top_level_sql_words(sql: &str) -> Vec<String> {
    let characters = sql.chars().collect::<Vec<_>>();
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut dollar_quote = None;
    let mut depth = 0_usize;
    let mut index = 0;
    while index < characters.len() {
        if let Some(delimiter) = dollar_quote.as_deref() {
            if characters[index..].starts_with(delimiter) {
                index += delimiter.len();
                dollar_quote = None;
            } else {
                index += 1;
            }
            continue;
        }
        let character = characters[index];
        match quote {
            Some(active) if character == active => {
                if characters.get(index + 1) == Some(&active) {
                    index += 2;
                } else {
                    quote = None;
                    index += 1;
                }
            }
            Some(_) => index += 1,
            None if matches!(character, '\'' | '"') => {
                push_sql_word(&mut words, &mut word);
                quote = Some(character);
                index += 1;
            }
            None if character == '$' => {
                if let Some(delimiter) = dollar_quote_delimiter(&characters, index) {
                    push_sql_word(&mut words, &mut word);
                    index += delimiter.len();
                    dollar_quote = Some(delimiter);
                } else {
                    push_sql_word(&mut words, &mut word);
                    index += 1;
                }
            }
            None if character == '(' => {
                push_sql_word(&mut words, &mut word);
                depth += 1;
                index += 1;
            }
            None if character == ')' => {
                if depth == 0 {
                    push_sql_word(&mut words, &mut word);
                }
                depth = depth.saturating_sub(1);
                index += 1;
            }
            None if depth == 0 && (character.is_alphanumeric() || character == '_') => {
                word.push(character.to_ascii_lowercase());
                index += 1;
            }
            None => {
                if depth == 0 {
                    push_sql_word(&mut words, &mut word);
                }
                index += 1;
            }
        }
    }
    push_sql_word(&mut words, &mut word);
    words
}

fn push_sql_word(words: &mut Vec<String>, word: &mut String) {
    if !word.is_empty() {
        words.push(std::mem::take(word));
    }
}

fn match_aws(invocation: &CommandInvocation) -> Option<MatchedRule> {
    if invocation.executable != "aws" {
        return None;
    }
    let (service, operation) = aws_service_operation(&invocation.arguments)?;
    if aws_read_only_operation(service, operation) {
        return None;
    }
    if service == "athena"
        && operation == "start-query-execution"
        && let Some(matched) = match_aws_athena_cli(&invocation.arguments)
    {
        return Some(matched);
    }
    if let Some(matched) =
        match_aws_structured_delete_cli(service, operation, &invocation.arguments)
    {
        return Some(matched);
    }
    let destructive = operation.starts_with("delete-")
        || operation.starts_with("terminate-")
        || operation.starts_with("deregister-")
        || operation.starts_with("batch-delete-")
        || aws_exact_destructive_operation(service, operation)
        || (service == "s3" && operation == "rb")
        || (service == "kms" && operation == "schedule-key-deletion")
        || (service == "s3" && operation == "rm")
        || (service == "s3"
            && operation == "mv"
            && !has_option(&invocation.arguments, "--dryrun", None))
        || (service == "s3"
            && operation == "sync"
            && has_option(&invocation.arguments, "--delete", None)
            && !has_option(&invocation.arguments, "--dryrun", None));
    destructive.then(|| aws_destructive_rule("command.arguments"))
}
fn aws_exact_destructive_operation(service: &str, operation: &str) -> bool {
    matches!(
        (service, operation),
        ("s3api" | "glacier", "abort-multipart-upload")
            | ("sqs", "purge-queue")
            | ("codeartifact", "dispose-package-versions")
            | ("ec2", "release-address" | "release-hosts")
            | (
                "organizations",
                "close-account"
                    | "remove-account-from-organization"
                    | "leave-organization"
                    | "disable-aws-service-access"
            )
            | ("account", "disable-region")
            | ("kms", "retire-grant" | "revoke-grant")
    )
}

fn match_aws_structured_delete_cli(
    service: &str,
    operation: &str,
    arguments: &[String],
) -> Option<MatchedRule> {
    let option = match (service, operation) {
        ("dynamodb", "batch-write-item") => "--request-items",
        ("route53", "change-resource-record-sets") => "--change-batch",
        _ => return None,
    };
    let value = option_value(arguments, &[option])
        .or_else(|| option_value(arguments, &["--cli-input-json"]))?;
    if is_file_reference(value) {
        return Some(aws_uninspectable_rule("command.arguments"));
    }
    let payload = serde_json::from_str::<Value>(value).ok()?;
    let destructive = if service == "dynamodb" {
        json_contains_key(&payload, "DeleteRequest")
    } else {
        json_contains_string_field(&payload, "Action", "DELETE")
    };
    destructive.then(|| aws_destructive_rule("command.arguments"))
}

fn json_contains_key(value: &Value, key: &str) -> bool {
    match value {
        Value::Object(object) => {
            object.contains_key(key) || object.values().any(|child| json_contains_key(child, key))
        }
        Value::Array(array) => array.iter().any(|child| json_contains_key(child, key)),
        _ => false,
    }
}

fn json_contains_string_field(value: &Value, key: &str, expected: &str) -> bool {
    match value {
        Value::Object(object) => {
            object
                .get(key)
                .and_then(Value::as_str)
                .is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
                || object
                    .values()
                    .any(|child| json_contains_string_field(child, key, expected))
        }
        Value::Array(array) => array
            .iter()
            .any(|child| json_contains_string_field(child, key, expected)),
        _ => false,
    }
}

fn aws_service_operation(arguments: &[String]) -> Option<(&str, &str)> {
    let mut index = 0;
    while let Some(argument) = arguments.get(index) {
        if argument == "--" {
            index += 1;
            break;
        }
        if !argument.starts_with('-') {
            break;
        }
        let option = argument
            .split_once('=')
            .map_or(argument.as_str(), |(name, _)| name);
        index += 1;
        if !argument.contains('=') && aws_global_option_takes_value(option) {
            index += usize::from(index < arguments.len());
        }
    }
    Some((
        arguments.get(index)?.as_str(),
        arguments.get(index + 1)?.as_str(),
    ))
}

fn aws_global_option_takes_value(option: &str) -> bool {
    matches!(
        option,
        "--endpoint-url"
            | "--output"
            | "--query"
            | "--profile"
            | "--region"
            | "--version"
            | "--color"
            | "--ca-bundle"
            | "--cli-read-timeout"
            | "--cli-connect-timeout"
            | "--cli-binary-format"
            | "--cli-error-format"
    )
}

fn aws_read_only_operation(service: &str, operation: &str) -> bool {
    operation.starts_with("describe-")
        || operation.starts_with("list-")
        || operation.starts_with("get-")
        || (service == "s3" && matches!(operation, "ls" | "cp"))
}

fn match_aws_athena_cli(arguments: &[String]) -> Option<MatchedRule> {
    for names in [
        &["--query-string"][..],
        &["--cli-input-json", "--cli-input-yaml"][..],
    ] {
        if let Some(value) = option_value(arguments, names)
            && is_file_reference(value)
        {
            return Some(aws_uninspectable_rule("command.arguments"));
        }
    }
    option_value(arguments, &["--query-string"])
        .filter(|sql| match_destructive_sql(sql, "command.arguments").is_some())
        .map(|_| aws_destructive_rule("command.arguments"))
}

fn is_file_reference(value: &str) -> bool {
    value
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file://"))
        || value
            .get(..8)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("fileb://"))
}

fn aws_destructive_rule(field: &str) -> MatchedRule {
    rule(
        "cloud.aws",
        "destructive-operation",
        field,
        "aws.destructive_operation",
        "Runs a destructive AWS operation",
        "Use a read-only describe command and follow the approved cloud change process",
    )
}

fn aws_uninspectable_rule(field: &str) -> MatchedRule {
    rule(
        "cloud.aws",
        "uninspectable-query",
        field,
        "aws.uninspectable_query",
        "Loads an AWS query from content that guardrails cannot inspect",
        "Provide the query inline so guardrails can inspect it before execution",
    )
}

fn match_aws_mcp(call: &McpCall) -> Option<MatchedRule> {
    if !server_has_identity(&call.server, &["aws", "amazon"]) {
        return None;
    }
    let tool = normalize_identity(&call.tool);
    if aws_read_only_identity(&tool) {
        return None;
    }
    if let Some(matched) = match_aws_athena_mcp(&tool, &call.arguments) {
        return Some(matched);
    }
    if aws_structured_mcp_delete(&tool, &call.arguments) {
        return Some(aws_destructive_rule("arguments"));
    }
    if aws_destructive_identity(&tool) {
        return Some(aws_destructive_rule("tool"));
    }
    for path in ["$.operation", "$.action", "$.method"] {
        let Some(operation) = first_string(&call.arguments, path) else {
            continue;
        };
        let operation = normalize_identity(operation);
        if aws_read_only_identity(&operation) {
            return None;
        }
        if let Some(matched) = match_aws_athena_mcp(&operation, &call.arguments) {
            return Some(matched);
        }
        if aws_destructive_identity(&operation) {
            return Some(aws_destructive_rule(path));
        }
    }
    None
}

fn aws_read_only_identity(identity: &str) -> bool {
    identity
        .split(['.', '_', '/', ':'])
        .any(|part| matches!(part, "describe" | "list" | "get"))
        || identity_has_sequence(identity, "s3", "ls")
        || identity_has_sequence(identity, "s3", "cp")
}

fn aws_destructive_identity(identity: &str) -> bool {
    ["delete", "terminate", "destroy", "remove", "deregister"]
        .iter()
        .any(|operation| identity_has_operation(identity, operation))
        || identity_has_sequence(identity, "s3", "rb")
        || identity.contains("schedule_key_deletion")
        || aws_exact_destructive_identity(identity)
}

fn aws_exact_destructive_identity(identity: &str) -> bool {
    let identity = flattened_identity(identity);
    [
        "s3_rm",
        "s3_mv",
        "abort_multipart_upload",
        "sqs_purge_queue",
        "dispose_package_versions",
        "ec2_release_address",
        "ec2_release_hosts",
        "organizations_close_account",
        "account_disable_region",
        "organizations_leave_organization",
        "retire_grant",
        "revoke_grant",
    ]
    .iter()
    .any(|candidate| identity.contains(candidate))
}

fn aws_structured_mcp_delete(identity: &str, arguments: &Value) -> bool {
    identity.contains("batch_write_item") && json_contains_key(arguments, "DeleteRequest")
        || identity.contains("change_resource_record_sets")
            && json_contains_string_field(arguments, "Action", "DELETE")
        || flattened_identity(identity).contains("s3_sync")
            && ["$.delete", "$.input.delete"]
                .iter()
                .any(|path| first_bool(arguments, path) == Some(true))
}

fn identity_has_sequence(identity: &str, first: &str, second: &str) -> bool {
    let mut previous = None;
    for part in identity.split(['.', '_', '/', ':']) {
        if previous == Some(first) && part == second {
            return true;
        }
        previous = Some(part);
    }
    false
}

fn match_aws_athena_mcp(identity: &str, arguments: &Value) -> Option<MatchedRule> {
    if !identity.contains("athena") || !identity.contains("start_query_execution") {
        return None;
    }
    for path in [
        "$.query_string",
        "$.queryString",
        "$.input.query_string",
        "$.input.queryString",
        "$.cli_input_json",
        "$.cliInputJson",
        "$.cli_input_yaml",
        "$.cliInputYaml",
    ] {
        if first_string(arguments, path).is_some_and(is_file_reference) {
            return Some(aws_uninspectable_rule(path));
        }
    }
    for path in [
        "$.query_string",
        "$.queryString",
        "$.query",
        "$.sql",
        "$.statement",
        "$.input.query_string",
        "$.input.queryString",
        "$.input.query",
        "$.input.sql",
    ] {
        if first_string(arguments, path)
            .is_some_and(|sql| match_destructive_sql(sql, path).is_some())
        {
            return Some(aws_destructive_rule(path));
        }
    }
    None
}

fn match_gcp(invocation: &CommandInvocation) -> Option<MatchedRule> {
    if invocation.executable != "gcloud" {
        return None;
    }
    let arguments = &invocation.arguments;
    let positional = positional_arguments(arguments);
    let destructive = positional.iter().any(|argument| argument == "delete")
        || has_string_sequence(&positional, &["storage", "rm"])
        || has_string_sequence(&positional, &["tasks", "queues", "purge"])
        || has_string_sequence(&positional, &["secrets", "versions", "destroy"])
        || has_string_sequence(&positional, &["kms", "keys", "versions", "destroy"])
        || has_string_sequence(&positional, &["projects", "remove-iam-policy-binding"])
        || has_string_sequence(
            &positional,
            &["iam", "service-accounts", "remove-iam-policy-binding"],
        )
        || (has_string_sequence(
            &positional,
            &["storage", "batch-operations", "jobs", "create"],
        ) && has_option(arguments, "--delete-object", None)
            && !has_option(arguments, "--dry-run", None));
    destructive.then(|| {
        rule(
            "cloud.gcp",
            "delete-resource",
            "command.arguments",
            "gcp.delete_resource",
            "Deletes a Google Cloud resource or access binding",
            "Describe the resource and follow the approved cloud change process",
        )
    })
}

fn has_string_sequence(arguments: &[String], sequence: &[&str]) -> bool {
    arguments.windows(sequence.len()).any(|window| {
        window
            .iter()
            .map(String::as_str)
            .eq(sequence.iter().copied())
    })
}
fn match_gcp_mcp(call: &McpCall) -> Option<MatchedRule> {
    if let Some(matched) = match_operation_mcp(
        call,
        &["gcp", "google_cloud", "google"],
        &["delete", "destroy", "remove"],
        "cloud.gcp",
        "delete-resource",
        "gcp.delete_resource",
        "Deletes a Google Cloud resource",
    ) {
        return Some(matched);
    }
    if !server_has_identity(&call.server, &["gcp", "google"]) {
        return None;
    }
    let tool = normalize_identity(&call.tool);
    if gcp_exact_destructive_identity(&tool) {
        return Some(rule(
            "cloud.gcp",
            "delete-resource",
            "tool",
            "gcp.delete_resource",
            "Deletes a Google Cloud resource or access binding",
            "Use a read-only operation and follow the approved change process",
        ));
    }
    for path in ["$.operation", "$.action", "$.method"] {
        if first_string(&call.arguments, path)
            .map(normalize_identity)
            .is_some_and(|identity| gcp_exact_destructive_identity(&identity))
        {
            return Some(rule(
                "cloud.gcp",
                "delete-resource",
                path,
                "gcp.delete_resource",
                "Deletes a Google Cloud resource or access binding",
                "Use a read-only operation and follow the approved change process",
            ));
        }
    }
    None
}

fn gcp_exact_destructive_identity(identity: &str) -> bool {
    let identity = flattened_identity(identity);
    [
        "storage_rm",
        "tasks_queues_purge",
        "secrets_versions_destroy",
        "kms_keys_versions_destroy",
        "remove_iam_policy_binding",
    ]
    .iter()
    .any(|candidate| identity.contains(candidate))
}

fn match_mcp_pack(pack: &str, call: &McpCall) -> Option<MatchedRule> {
    if matches!(pack, "core.shell" | "core.git" | "core.filesystem")
        && let Some(matched) = match_shell_tool(pack, &call.tool, &call.arguments)
    {
        return Some(matched);
    }
    match pack {
        "database.postgresql" => match_postgresql_mcp(call),
        "cloud.aws" => match_aws_mcp(call),
        "cloud.gcp" => match_gcp_mcp(call),
        "saas.github" => github::match_call(call),
        "saas.notion" => match_notion(call),
        _ => None,
    }
}

fn match_postgresql_mcp(call: &McpCall) -> Option<MatchedRule> {
    if !postgresql_server(&call.server) {
        return None;
    }
    let tool = normalize_identity(&call.tool);
    if matches!(
        tool.as_str(),
        "delete_table"
            | "drop_table"
            | "alter_table_drop_column"
            | "drop_index"
            | "drop_view"
            | "drop_function"
            | "drop_trigger"
            | "drop_schema"
    ) {
        return Some(rule(
            "database.postgresql",
            "structured-destructive-operation",
            "tool",
            "postgresql.structured_destructive_operation",
            "Deletes PostgreSQL data or a database object",
            "Inspect dependencies and use a reviewed migration",
        ));
    }
    if !matches!(
        tool.as_str(),
        "execute_sql" | "run_sql" | "query" | "postgres.execute" | "postgresql.execute"
    ) {
        return None;
    }
    for path in [
        "$.sql",
        "$.query",
        "$.statement",
        "$.sqlStatement",
        "$.input.sql",
        "$.input.sqlStatement",
    ] {
        if let Some(sql) = first_string(&call.arguments, path)
            && let Some(matched) = match_destructive_sql(sql, path)
        {
            return Some(matched);
        }
    }
    None
}

fn postgresql_server(server: &str) -> bool {
    let normalized = normalize_identity(server);
    server_has_identity(server, &["postgres", "postgresql", "pg", "cloudsql"])
        || (normalized
            .split(['.', '_', '/', ':'])
            .any(|part| part == "cloud")
            && normalized
                .split(['.', '_', '/', ':'])
                .any(|part| part == "sql"))
}

fn match_operation_mcp(
    call: &McpCall,
    server_identities: &[&str],
    destructive_terms: &[&str],
    pack: &str,
    rule_id: &str,
    reason_code: &str,
    description: &str,
) -> Option<MatchedRule> {
    if !server_has_identity(&call.server, server_identities) {
        return None;
    }
    let tool = normalize_identity(&call.tool);
    let tool_matches = destructive_terms
        .iter()
        .any(|term| identity_has_operation(&tool, term));
    if tool_matches {
        return Some(rule(
            pack,
            rule_id,
            "tool",
            reason_code,
            description,
            "Use a read-only operation and follow the approved change process",
        ));
    }
    for path in ["$.operation", "$.action", "$.method"] {
        if let Some(operation) = first_string(&call.arguments, path)
            && destructive_terms
                .iter()
                .any(|term| identity_has_operation(&normalize_identity(operation), term))
        {
            return Some(rule(
                pack,
                rule_id,
                path,
                reason_code,
                description,
                "Use a read-only operation and follow the approved change process",
            ));
        }
    }
    None
}

fn match_notion(call: &McpCall) -> Option<MatchedRule> {
    if !server_has_identity(&call.server, &["notion"]) {
        return None;
    }
    let tool = normalize_identity(&call.tool);
    let destructive_tool = [
        "delete_page",
        "archive_page",
        "move_page",
        "delete_database",
        "archive_database",
        "move_database",
        "delete_workspace",
        "archive_workspace",
        "pages.delete",
        "pages.archive",
        "pages.move",
        "databases.delete",
        "databases.archive",
        "databases.move",
        "workspaces.delete",
        "workspaces.archive",
        "notion_move_pages",
    ]
    .iter()
    .any(|candidate| tool == *candidate);
    if destructive_tool {
        return Some(rule(
            "saas.notion",
            notion_rule_id(&tool),
            "tool",
            "notion.destructive_operation",
            "Changes or removes a Notion page, database, or workspace",
            "Duplicate or export the object before an operator-approved change",
        ));
    }

    let update_alias = matches!(
        tool.as_str(),
        "update_page"
            | "pages.update"
            | "update_database"
            | "databases.update"
            | "update_workspace"
            | "notion_update_page"
            | "notion_update_data_source"
    );
    if update_alias {
        for path in [
            "$.archived",
            "$.is_archived",
            "$.in_trash",
            "$.deleted",
            "$.erase_content",
            "$.allow_deleting_content",
        ] {
            if first_bool(&call.arguments, path) == Some(true) {
                return Some(rule(
                    "saas.notion",
                    "archive-object",
                    path,
                    "notion.archive_object",
                    "Archives or deletes a Notion object",
                    "Duplicate or export the object before an operator-approved archive",
                ));
            }
        }
        if first_string(&call.arguments, "$.command")
            .is_some_and(|command| normalize_identity(command) == "replace_content")
        {
            return Some(rule(
                "saas.notion",
                "replace-page-content",
                "$.command",
                "notion.replace_page_content",
                "Replaces the current page content",
                "Duplicate or export the page before replacing its content",
            ));
        }
        for path in ["$.action", "$.operation"] {
            if first_string(&call.arguments, path).is_some_and(|operation| {
                matches!(
                    normalize_identity(operation).as_str(),
                    "delete" | "archive" | "move" | "trash"
                )
            }) {
                return Some(rule(
                    "saas.notion",
                    "destructive-update",
                    path,
                    "notion.destructive_update",
                    "Moves, archives, or deletes a Notion object",
                    "Duplicate or export the object before an operator-approved change",
                ));
            }
        }
    }
    None
}

fn match_notion_cli(invocation: &CommandInvocation) -> Option<MatchedRule> {
    if invocation.executable != "ntn" {
        return None;
    }
    let arguments = &invocation.arguments;
    let destructive = has_string_sequence(arguments, &["pages", "trash"])
        || has_string_sequence(arguments, &["pages", "edit"])
            && has_option(arguments, "--allow-deleting-content", None)
        || has_string_sequence(arguments, &["workers", "delete"])
        || has_string_sequence(arguments, &["workers", "rm"])
        || has_string_sequence(arguments, &["workers", "env", "unset"])
        || has_string_sequence(arguments, &["workers", "env", "delete"])
        || has_string_sequence(arguments, &["workers", "env", "rm"])
        || has_string_sequence(arguments, &["workers", "databases", "attach"])
            && has_option(arguments, "--yes", Some('y'))
        || has_string_sequence(arguments, &["api"]) && notion_api_request_is_destructive(arguments);
    destructive.then(|| {
        rule(
            "saas.notion",
            "cli-destructive-operation",
            "command.arguments",
            "notion.cli_destructive_operation",
            "Deletes, trashes, or replaces Notion content or worker state",
            "Export the target and review the exact API mutation first",
        )
    })
}

fn notion_api_request_is_destructive(arguments: &[String]) -> bool {
    option_value(arguments, &["-X", "--request"])
        .is_some_and(|method| method.eq_ignore_ascii_case("DELETE"))
        || arguments.iter().any(|argument| {
            let compact = argument.to_ascii_lowercase().replace(' ', "");
            compact.contains("in_trash:=true")
                || compact.contains("\"in_trash\":true")
                || compact.contains("erase_content:=true")
                || compact.contains("\"erase_content\":true")
                || compact.contains("\"properties\"") && compact.contains(":null")
        })
}

fn notion_rule_id(tool: &str) -> &'static str {
    if tool.contains("workspace") {
        "destructive-workspace"
    } else if tool.contains("database") {
        "destructive-database"
    } else if tool.contains("move") {
        "move-object"
    } else {
        "destructive-page"
    }
}

fn first_string<'a>(value: &'a Value, path: &str) -> Option<&'a str> {
    JsonPath::parse(path)
        .ok()?
        .select(value)
        .into_iter()
        .find_map(Value::as_str)
}

fn first_bool(value: &Value, path: &str) -> Option<bool> {
    JsonPath::parse(path)
        .ok()?
        .select(value)
        .into_iter()
        .find_map(Value::as_bool)
}

fn normalize_identity(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('-', "_")
}
fn flattened_identity(value: &str) -> String {
    normalize_identity(value)
        .chars()
        .map(|character| {
            if matches!(character, '.' | '/' | ':') {
                '_'
            } else {
                character
            }
        })
        .collect()
}

fn identity_has_operation(identity: &str, operation: &str) -> bool {
    identity
        .split(['.', '_', '/', ':'])
        .any(|part| part == operation)
        || identity.starts_with(&format!("{operation}_"))
}

fn server_has_identity(server: &str, identities: &[&str]) -> bool {
    let server = normalize_identity(server);
    server
        .split(['.', '_', '/', ':'])
        .any(|part| identities.contains(&part))
}

fn git_operation(arguments: &[String]) -> Option<&str> {
    let mut index = 0;
    while let Some(argument) = arguments.get(index) {
        if argument == "--" {
            return arguments.get(index + 1).map(String::as_str);
        }
        let option = argument
            .split_once('=')
            .map_or(argument.as_str(), |(name, _)| name);
        if matches!(
            option,
            "-C" | "-c" | "--git-dir" | "--work-tree" | "--namespace" | "--super-prefix"
        ) {
            index += if argument.contains('=') { 1 } else { 2 };
            continue;
        }
        if argument.starts_with('-') {
            index += 1;
            continue;
        }
        return Some(argument);
    }
    None
}

fn sql_statements(sql: &str) -> Vec<String> {
    let characters = sql.chars().collect::<Vec<_>>();
    let mut statements = Vec::new();
    let mut statement = String::new();
    let mut quote = None;
    let mut dollar_quote = None;
    let mut index = 0;
    while index < characters.len() {
        if let Some(delimiter) = dollar_quote.as_deref() {
            if characters[index..].starts_with(delimiter) {
                statement.extend(delimiter);
                index += delimiter.len();
                dollar_quote = None;
            } else {
                statement.push(characters[index]);
                index += 1;
            }
            continue;
        }
        let character = characters[index];
        match quote {
            Some(active) if character == active => {
                statement.push(character);
                if characters.get(index + 1) == Some(&active) {
                    statement.push(active);
                    index += 2;
                } else {
                    quote = None;
                    index += 1;
                }
            }
            Some(_) => {
                statement.push(character);
                index += 1;
            }
            None if character == '-' && characters.get(index + 1) == Some(&'-') => {
                index += 2;
                while index < characters.len() && characters[index] != '\n' {
                    index += 1;
                }
                statement.push(' ');
            }
            None if character == '/' && characters.get(index + 1) == Some(&'*') => {
                let mut depth = 1;
                index += 2;
                while index < characters.len() && depth > 0 {
                    if characters[index..].starts_with(&['/', '*']) {
                        depth += 1;
                        index += 2;
                    } else if characters[index..].starts_with(&['*', '/']) {
                        depth -= 1;
                        index += 2;
                    } else {
                        index += 1;
                    }
                }
                statement.push(' ');
            }
            None if matches!(character, '\'' | '"') => {
                quote = Some(character);
                statement.push(character);
                index += 1;
            }
            None if character == '$' => {
                if let Some(delimiter) = dollar_quote_delimiter(&characters, index) {
                    statement.extend(delimiter.iter());
                    index += delimiter.len();
                    dollar_quote = Some(delimiter);
                } else {
                    statement.push(character);
                    index += 1;
                }
            }
            None if character == ';' => {
                if !statement.trim().is_empty() {
                    statements.push(std::mem::take(&mut statement));
                }
                index += 1;
            }
            None => {
                statement.push(character);
                index += 1;
            }
        }
    }
    if !statement.trim().is_empty() {
        statements.push(statement);
    }
    statements
}

fn dollar_quote_delimiter(characters: &[char], start: usize) -> Option<Vec<char>> {
    let end = characters[start + 1..]
        .iter()
        .position(|character| *character == '$')?
        + start
        + 1;
    let tag = &characters[start + 1..end];
    if !tag.is_empty()
        && (!matches!(tag.first(), Some(character) if character.is_ascii_alphabetic() || *character == '_')
            || !tag
                .iter()
                .all(|character| character.is_ascii_alphanumeric() || *character == '_'))
    {
        return None;
    }
    Some(characters[start..=end].to_vec())
}

fn positional_arguments(arguments: &[String]) -> Vec<String> {
    arguments
        .iter()
        .filter(|argument| !argument.starts_with('-'))
        .cloned()
        .collect()
}

fn has_option(arguments: &[String], long: &str, short: Option<char>) -> bool {
    arguments
        .iter()
        .take_while(|argument| argument.as_str() != "--")
        .any(|argument| {
            argument == long
                || argument.strip_prefix(&format!("{long}=")).is_some()
                || short.is_some_and(|short| {
                    argument.starts_with('-')
                        && !argument.starts_with("--")
                        && argument[1..].chars().any(|candidate| candidate == short)
                })
        })
}
pub(super) fn subcommand_index(
    arguments: &[String],
    options_with_values: &[&str],
) -> Option<usize> {
    let mut index = 0;
    while index < arguments.len() {
        let argument = &arguments[index];
        if argument == "--" {
            return (index + 1 < arguments.len()).then_some(index + 1);
        }
        if !argument.starts_with('-') || argument == "-" {
            return Some(index);
        }
        let takes_separate_value =
            !argument.contains('=') && options_with_values.contains(&argument.as_str());
        index += if takes_separate_value { 2 } else { 1 };
    }
    None
}

fn option_value<'a>(arguments: &'a [String], names: &[&str]) -> Option<&'a str> {
    for (index, argument) in arguments.iter().enumerate() {
        if names.contains(&argument.as_str()) {
            return arguments.get(index + 1).map(String::as_str);
        }
        for name in names {
            if let Some(value) = argument.strip_prefix(&format!("{name}=")) {
                return Some(value);
            }
        }
    }
    None
}

fn rule(
    pack_id: &str,
    rule_id: &str,
    matched_field: &str,
    reason_code: &str,
    description: &str,
    safer_action: &str,
) -> MatchedRule {
    MatchedRule {
        pack_id: pack_id.to_string(),
        rule_id: rule_id.to_string(),
        matched_field: matched_field.to_string(),
        reason_code: ReasonCode::new(reason_code).expect("static reason code is valid"),
        description: description.to_string(),
        safer_action: safer_action.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        time::{Duration, Instant},
    };

    use serde_json::json;

    use super::*;
    use crate::{EffectiveScope, GuardPhase, PolicyMode};

    fn evaluate(pack: &str, payload: EvaluationPayload) -> Option<MatchedRule> {
        BuiltInEvaluator
            .evaluate(
                &EvaluationInput::new(GuardPhase::HarnessPreTool, payload),
                &EffectivePolicy {
                    enabled: true,
                    mode: PolicyMode::Deny,
                    packs: vec![PackId::new(pack).unwrap()],
                    managed_checks: Vec::new(),
                    stream_buffer_bytes: 1024,
                    stream_buffer_timeout_ms: 1_000,
                    scope: EffectiveScope::Global,
                },
            )
            .unwrap()
    }

    #[test]
    fn every_shell_pack_covers_positive_quoting_order_data_and_near_miss_cases() {
        let cases = vec![
            (
                "core.shell",
                vec![
                    "reboot",
                    "bash -c 'poweroff'",
                    "kill 1 -9",
                    "printf 'rm -rf /tmp/work' | bash",
                    "systemctl poweroff",
                    "systemctl start reboot.target",
                    "init 6",
                    "kill -9 -1",
                ],
                vec![
                    "echo 'reboot'",
                    "reboot-check",
                    "kill 10 -9",
                    "systemctl status",
                    "kill -1 1234",
                ],
            ),
            (
                "core.git",
                vec![
                    "git reset --hard",
                    "bash -c 'git branch -D old'",
                    "git --no-pager reset main --hard",
                    "git -C /repo reset --hard",
                    "git --git-dir=/repo/.git reset --hard",
                    "git push origin --delete old",
                    "git push origin :old",
                    "git branch --delete --force old",
                    "git branch -df old",
                    "git branch -d merged",
                    "git checkout --force main",
                    "git switch --discard-changes main",
                    "git push --mirror origin",
                    "git push origin +main",
                    "git worktree remove --force ../dirty",
                    "git submodule update --force",
                    "git prune",
                    "git repack --cruft --cruft-expiration=now -d",
                    "git checkout main -- src/lib.rs",
                    "git checkout -- src/lib.rs",
                    "git clean -f",
                ],
                vec![
                    "printf '%s' 'git reset --hard'",
                    "git reset --soft HEAD~1",
                    "git checkout -b feature main",
                    "git checkout feature/login",
                    "git clean -n -f",
                    "git clean -nf",
                ],
            ),
            (
                "core.filesystem",
                vec![
                    "rm -rf /tmp/work",
                    "bash -c 'find . -delete'",
                    "printf reset >| state.txt",
                    "printf reset &> state.txt",
                    "rm --force --recursive /tmp/work",
                    "rm -Rf /tmp/work",
                    "rm -R -f /tmp/work",
                    "nice rm -rf /tmp/work",
                    "setsid rm -rf /tmp/work",
                    "chroot /sandbox rm -rf /tmp/work",
                    "cmd=rm; $cmd -rf /tmp/work",
                    "opts=-rf; rm $opts /tmp/work",
                    "find /tmp/work -exec rm -rf {} +",
                    "function wipe { rm -rf /tmp/work; }; wipe",
                    "sudo FOO=bar rm -rf /tmp/work",
                    "2>&1 rm -rf /tmp/work",
                    "rm -r /tmp/work",
                    "unlink report.txt",
                    "shred secrets.txt",
                    "truncate -s 0 audit.log",
                    "truncate -s 0K audit.log",
                    "truncate --size=-1G database.bin",
                    "dd if=image.raw of=/dev/disk4",
                    "wipefs --all /dev/sdb",
                    "diskutil partitionDisk disk4 GPT APFS Data 0b",
                    "printf reset > state.txt",
                    "find /tmp/work -ok rm -r {} ;",
                    "printf reset >& state.txt",
                    "printf reset >&state.txt",
                ],
                vec![
                    "echo 'rm -rf /tmp/work'",
                    "rm -f report.txt",
                    "rm -- -rf",
                    "find . -depth",
                    "printf ok >> state.txt",
                    "dd if=input of=output",
                    "truncate -s 10 report.txt",
                    "truncate -s 1K report.txt",
                    "printf ok >&2",
                    "[[ alpha > beta ]]",
                    "(( count > 1 ))",
                    "exec 3<>state.txt",
                    "cat <<'EOF'\nnot > a redirect\nEOF",
                    "printf ok >/dev/null",
                    "printf ok 2>/dev/stderr",
                ],
            ),
            (
                "database.postgresql",
                vec![
                    "psql -c 'DROP DATABASE app'",
                    "bash -c \"psql -c 'TRUNCATE audit'\"",
                    "psql --host=db --command='DELETE FROM users'",
                    "psql -c 'SELECT 1; DROP DATABASE app'",
                    "psql -c '-- migration\nDROP TABLE users'",
                    "psql -c '/* migration */ DROP TABLE users'",
                    "psql -c 'DROP /* migration */ TABLE users'",
                    "psql -c 'WITH x AS (SELECT 1) DELETE FROM users'",
                    "psql -c 'EXPLAIN ANALYZE DELETE FROM users'",
                    "psql -c 'DROP VIEW current_users'",
                    "psql -c 'ALTER TABLE users DROP COLUMN legacy'",
                    "psql -f migration.sql",
                    "cat migration.sql | psql",
                    "printf 'DROP TABLE users' | sudo -u postgres psql",
                ],
                vec![
                    "echo 'DROP DATABASE app'",
                    "psql -c 'DELETE FROM users WHERE id = 1'",
                    "psql -c 'SELECT drop_database_hint FROM docs'",
                    "psql -c 'WITH x AS (SELECT 1) SELECT * FROM x'",
                    "psql -c 'EXPLAIN SELECT \"delete\" FROM docs'",
                    "printf safe | cat",
                ],
            ),
            (
                "database.snowflake",
                vec![
                    "snow sql -q 'DROP DATABASE app'",
                    "snow sql --query='DELETE FROM users'",
                    "snow sql -f migration.sql",
                    "snow object drop database app",
                    "snow stage remove @archive/path",
                    "snow git drop analytics",
                    "snow dcm purge",
                    "snow dbt deploy analytics --force",
                    "snow stage copy local @stage --overwrite",
                    "snow streamlit deploy app --prune",
                    "snow spcs compute-pool stop-all pool",
                    "snow sql -q 'WITH x AS (SELECT 1) UPDATE users SET active = false'",
                    "snow sql -q 'WITH x AS (SELECT 1) DELETE FROM users'",
                    "snow sql -q 'BEGIN DELETE FROM users; END;'",
                    "snow sql -q \"$QUERY\"",
                    "snow sql -q 'ALTER TABLE users DROP COLUMN email'",
                    "snow sql -q 'ALTER TABLE IF EXISTS users RENAME TO former_users'",
                    "snow sql -q '!abort 01b123'",
                    "snow sql -q '!edit'",
                ],
                vec![
                    "snow sql -q 'SELECT * FROM users'",
                    "snow object list database",
                    "echo 'snow object drop database app'",
                    "snow sql -q 'BEGIN SELECT 1; END;'",
                ],
            ),
            (
                "cloud.aws",
                vec![
                    "aws ec2 terminate-instances --instance-ids i-1",
                    "bash -c 'aws s3 rm s3://bucket --recursive'",
                    "aws --region us-east-1 ec2 delete-vpc --vpc-id vpc-1",
                    "aws s3 rb s3://prod-bucket",
                    "aws kms schedule-key-deletion --key-id key-1",
                    "aws ecr batch-delete-image --repository-name app --image-ids imageTag=old",
                    "aws glue batch-delete-table --database-name prod --tables-to-delete old",
                    "aws glue batch-delete-partition --database-name prod --table-name events",
                    "aws athena start-query-execution --query-string 'DROP TABLE prod.customers'",
                    "aws athena start-query-execution --query-string file:///tmp/query.sql",
                    "aws athena start-query-execution --cli-input-json file://request.json",
                    "aws s3 rm s3://bucket/path",
                    "aws s3 mv s3://bucket/old s3://bucket/new",
                    "aws s3 sync ./out s3://bucket --delete",
                    "aws sqs purge-queue --queue-url https://example.invalid/queue",
                    "aws organizations close-account --account-id 123456789012",
                    "aws dynamodb batch-write-item --request-items '{\"Users\":[{\"DeleteRequest\":{\"Key\":{\"id\":{\"S\":\"1\"}}}}]}'",
                    "aws route53 change-resource-record-sets --hosted-zone-id Z1 --change-batch '{\"Changes\":[{\"Action\":\"DELETE\"}]}'",
                    "aws dynamodb batch-write-item --cli-input-json file://delete.json",
                ],
                vec![
                    "echo 'aws ec2 terminate-instances'",
                    "aws ec2 describe-instances",
                    "aws s3 ls s3://bucket",
                    "aws s3 cp local.txt s3://bucket/local.txt",
                    "aws ec2 describe-instances --filters Name=tag:job,Values=delete-old",
                    "aws s3api list-buckets --cli-input-json delete-me.json",
                    "aws --profile delete-old ec2 describe-instances",
                    "aws athena start-query-execution --query-string 'SELECT * FROM prod.customers'",
                    "aws athena start-query-execution --query-string 'DELETE FROM prod.customers WHERE id = 1'",
                    "aws dynamodb batch-write-item --cli-input-json '{\"RequestItems\":{\"Users\":[{\"PutRequest\":{\"Item\":{\"id\":{\"S\":\"1\"}}}}]}}'",
                ],
            ),
            (
                "cloud.gcp",
                vec![
                    "gcloud projects delete example",
                    "bash -c 'gcloud compute instances delete vm'",
                    "gcloud --quiet compute disks delete disk",
                    "gcloud storage rm gs://bucket/object",
                    "gcloud tasks queues purge queue",
                    "gcloud secrets versions destroy 2 --secret=api",
                    "gcloud projects remove-iam-policy-binding project --member=user:a@example.com",
                ],
                vec![
                    "echo 'gcloud projects delete example'",
                    "gcloud projects describe example",
                    "gcloud compute instances list",
                ],
            ),
            (
                "kubernetes.kubectl",
                vec![
                    "kubectl delete namespace prod",
                    "kubectl delete pods --all",
                    "kubectl drain node-1",
                    "kubectl scale deployment/api --replicas=0",
                    "kubectl apply -f app.yaml --force",
                    "kubectl delete pod api-1",
                    "kubectl delete -f old.yaml",
                    "kubectl delete pod api-1 --force",
                    "kubectl apply --prune -f app.yaml -l app=api",
                    "kubectl replace --force -f app.yaml",
                    "kubectl auth reconcile -f rbac.yaml --remove-extra-subjects",
                    "kubectl --context prod delete deploy api",
                    "kubectl delete deploy api",
                    "kubectl delete sts db",
                    "kubectl delete ds agent",
                ],
                vec![
                    "kubectl get pods",
                    "kubectl delete namespace prod --dry-run=client",
                    "kubectl apply -f app.yaml --force --dry-run=server",
                    "kubectl get namespace delete",
                    "kubectl -n prod get pod delete",
                    "kubectl drain node-1 --dry-run=client",
                    "kubectl cordon node-1 --dry-run=client",
                    "kubectl taint node-1 maintenance=true:NoExecute --dry-run=client",
                    "kubectl scale deployment/api --replicas=0 --dry-run=server",
                ],
            ),
            (
                "kubernetes.helm",
                vec![
                    "helm uninstall api",
                    "helm rollback api 2",
                    "helm upgrade api ./chart --force",
                    "helm upgrade api ./chart --reset-values",
                    "helm del api",
                    "helm un api",
                    "helm upgrade api ./chart --force-replace",
                    "helm install api ./chart --cleanup-on-fail",
                    "helm --namespace prod uninstall api",
                ],
                vec![
                    "helm status api",
                    "helm uninstall api --dry-run",
                    "helm upgrade api ./chart --force --dry-run=client",
                    "helm status uninstall",
                    "helm --namespace prod status uninstall",
                ],
            ),
            (
                "secrets.aws_secrets",
                vec![
                    "aws secretsmanager delete-secret --secret-id prod/api",
                    "aws secretsmanager update-secret --secret-id prod/api",
                    "aws ssm delete-parameters --names /prod/api /prod/db",
                    "aws secretsmanager rotate-secret --secret-id prod/api",
                    "aws secretsmanager update-secret-version-stage --secret-id prod/api",
                    "aws ssm put-parameter --name /prod/api --value next --overwrite",
                    "aws ssm delete-resource-policy --resource-arn arn:aws:ssm:us-east-1:123:parameter/prod/api",
                ],
                vec![
                    "aws secretsmanager describe-secret --secret-id prod/api",
                    "aws ssm get-parameter --name /prod/api",
                ],
            ),
            (
                "secrets.onepassword",
                vec![
                    "op item delete 'Database Password'",
                    "op vault delete Production",
                    "op connect token delete abc123",
                    "op document edit Architecture --file architecture.pdf",
                    "op item move 'Database Password' --vault Archive",
                    "op user suspend alice@example.com",
                    "op vault user revoke Production alice@example.com",
                    "op connect server delete production",
                ],
                vec!["op item get 'Database Password'", "op vault list"],
            ),
            (
                "secret_disclosure",
                vec![
                    "infisical secrets get API_KEY --plain",
                    "op read op://prod/api/key",
                    "doppler secrets download",
                    "vault kv get secret/prod/api",
                    "aws ssm get-parameter --name /prod/api --with-decryption",
                    "op inject -i template.env",
                    "op service-account create deploy",
                    "doppler secrets",
                    "doppler secrets substitute template.env",
                    "vault login token",
                    "vault operator init",
                    "aws secretsmanager get-random-password",
                    "op item get help",
                    "infisical secrets get help",
                    "op item get -- --help",
                ],
                vec![
                    "infisical run -- npm start",
                    "infisical secrets set API_KEY=new-value",
                    "infisical secrets delete OLD_KEY",
                    "infisical secrets folders get --path=/apps",
                    "op read --help",
                    "op run -- node server.js",
                    "aws ssm get-parameter --name /prod/api",
                ],
            ),
            (
                "saas.notion",
                vec![
                    "ntn pages trash page-id",
                    "ntn pages edit page-id --allow-deleting-content",
                    "ntn workers delete worker-id",
                    "ntn workers env unset API_KEY",
                    "ntn workers databases attach db primary --yes",
                    "ntn api v1/pages/page-id -X DELETE",
                ],
                vec![
                    "ntn pages get page-id",
                    "ntn workers list",
                    "ntn api v1/pages/page-id -X GET",
                ],
            ),
        ];

        for (pack, destructive, safe) in cases {
            for command in destructive {
                assert!(
                    evaluate(
                        pack,
                        EvaluationPayload::ShellCommand {
                            command: command.into()
                        }
                    )
                    .is_some(),
                    "{pack} did not match `{command}`"
                );
            }
            for command in safe {
                assert!(
                    evaluate(
                        pack,
                        EvaluationPayload::ShellCommand {
                            command: command.into()
                        }
                    )
                    .is_none(),
                    "{pack} matched safe command `{command}`"
                );
            }
        }
    }

    #[test]
    fn postgresql_dollar_quoted_text_is_not_matched_as_sql() {
        for command in [
            "psql -c 'SELECT $$safe; DROP TABLE users;$$'",
            "psql -c 'SELECT $body$safe; TRUNCATE audit;$body$'",
        ] {
            assert!(
                evaluate(
                    "database.postgresql",
                    EvaluationPayload::ShellCommand {
                        command: command.into(),
                    }
                )
                .is_none(),
                "matched SQL inside dollar-quoted text: {command}"
            );
        }
    }

    #[test]
    fn notion_pack_covers_aliases_key_order_data_and_near_misses() {
        for call in [
            McpCall {
                server: "notion-team".into(),
                tool: "pages.update".into(),
                arguments: json!({"page_id": "p", "archived": true}),
            },
            McpCall {
                server: "notion-team".into(),
                tool: "update_page".into(),
                arguments: json!({"archived": true, "page_id": "p"}),
            },
            McpCall {
                server: "notion-team".into(),
                tool: "databases.archive".into(),
                arguments: json!({"database_id": "d"}),
            },
            McpCall {
                server: "notion-team".into(),
                tool: "delete_workspace".into(),
                arguments: json!({"workspace_id": "w"}),
            },
            McpCall {
                server: "notion-team".into(),
                tool: "pages.move".into(),
                arguments: json!({"page_id": "p", "parent_id": "next"}),
            },
            McpCall {
                server: "notion-mcp".into(),
                tool: "notion-update-page".into(),
                arguments: json!({"page_id": "p", "command": "replace_content"}),
            },
            McpCall {
                server: "notion-mcp".into(),
                tool: "notion-update-data-source".into(),
                arguments: json!({"data_source_id": "d", "in_trash": true}),
            },
        ] {
            assert!(evaluate("saas.notion", EvaluationPayload::McpCall { call }).is_some());
        }

        for call in [
            McpCall {
                server: "notion-team".into(),
                tool: "search".into(),
                arguments: json!({"query": "archive_page"}),
            },
            McpCall {
                server: "notion-team".into(),
                tool: "pages.update".into(),
                arguments: json!({"archived": false, "note": "\"archive\""}),
            },
            McpCall {
                server: "notion-team".into(),
                tool: "archive_page_preview".into(),
                arguments: json!({"page_id": "p"}),
            },
            McpCall {
                server: "generic-content".into(),
                tool: "delete_page".into(),
                arguments: json!({"page_id": "secret-page"}),
            },
        ] {
            assert!(evaluate("saas.notion", EvaluationPayload::McpCall { call }).is_none());
        }
    }
    #[test]
    fn github_pack_covers_exact_tools_typed_methods_and_near_misses() {
        for call in [
            McpCall {
                server: "github-mcp-server".into(),
                tool: "delete_repository".into(),
                arguments: json!({"owner": "acme", "repo": "api"}),
            },
            McpCall {
                server: "github".into(),
                tool: "delete_file".into(),
                arguments: json!({"owner": "acme", "repo": "api", "path": "old.txt"}),
            },
            McpCall {
                server: "github".into(),
                tool: "create_or_update_file".into(),
                arguments: json!({"path": "config.yml", "sha": "abc123"}),
            },
            McpCall {
                server: "github".into(),
                tool: "actions_run_trigger".into(),
                arguments: json!({"method": "delete_workflow_run_logs"}),
            },
            McpCall {
                server: "github".into(),
                tool: "merge_pull_request".into(),
                arguments: json!({"pull_number": 7}),
            },
            McpCall {
                server: "github".into(),
                tool: "update_pull_request_state".into(),
                arguments: json!({"pull_number": 7, "state": "closed"}),
            },
            McpCall {
                server: "github".into(),
                tool: "delete_pending_pull_request_review".into(),
                arguments: json!({"pull_number": 7}),
            },
            McpCall {
                server: "github".into(),
                tool: "projects_write".into(),
                arguments: json!({"method": "delete_project_view"}),
            },
            McpCall {
                server: "github".into(),
                tool: "label_write".into(),
                arguments: json!({"method": "delete", "name": "obsolete"}),
            },
            McpCall {
                server: "github".into(),
                tool: "discussion_comment_write".into(),
                arguments: json!({"method": "delete", "comment_id": "1"}),
            },
            McpCall {
                server: "github".into(),
                tool: "issue_write".into(),
                arguments: json!({"method": "update", "issue_fields": [{"id": "f", "delete": true}]}),
            },
            McpCall {
                server: "github".into(),
                tool: "update_issue_state".into(),
                arguments: json!({"issue_number": 9, "state": "closed"}),
            },
            McpCall {
                server: "github".into(),
                tool: "sub_issue_write".into(),
                arguments: json!({"method": "remove"}),
            },
            McpCall {
                server: "github".into(),
                tool: "set_issue_fields".into(),
                arguments: json!({"fields": [{"id": "f", "delete": true}]}),
            },
        ] {
            assert!(evaluate("saas.github", EvaluationPayload::McpCall { call }).is_some());
        }

        for call in [
            McpCall {
                server: "github".into(),
                tool: "actions_run_trigger".into(),
                arguments: json!({"method": "run_workflow"}),
            },
            McpCall {
                server: "github".into(),
                tool: "create_or_update_file".into(),
                arguments: json!({"path": "new.txt"}),
            },
            McpCall {
                server: "github".into(),
                tool: "update_pull_request_state".into(),
                arguments: json!({"state": "open"}),
            },
            McpCall {
                server: "generic-content".into(),
                tool: "delete_repository".into(),
                arguments: json!({"repo": "api"}),
            },
        ] {
            assert!(evaluate("saas.github", EvaluationPayload::McpCall { call }).is_none());
        }
    }

    #[test]
    fn infrastructure_mcp_packs_use_server_tool_and_typed_arguments() {
        let cases = [
            (
                "database.postgresql",
                McpCall {
                    server: "team-postgresql".into(),
                    tool: "execute_sql".into(),
                    arguments: json!({"input": {"sql": "DROP TABLE audit"}}),
                },
                "$.input.sql",
            ),
            (
                "cloud.aws",
                McpCall {
                    server: "production-aws".into(),
                    tool: "resource.execute".into(),
                    arguments: json!({"resource": "secret-value", "action": "terminate_instance"}),
                },
                "$.action",
            ),
            (
                "cloud.gcp",
                McpCall {
                    server: "gcp-platform".into(),
                    tool: "compute.delete".into(),
                    arguments: json!({"name": "secret-value"}),
                },
                "tool",
            ),
            (
                "database.postgresql",
                McpCall {
                    server: "cloud-sql-postgres".into(),
                    tool: "drop_schema".into(),
                    arguments: json!({"schema": "archive"}),
                },
                "tool",
            ),
            (
                "cloud.aws",
                McpCall {
                    server: "production-aws".into(),
                    tool: "sqs.purge_queue".into(),
                    arguments: json!({"queue_url": "secret-value"}),
                },
                "tool",
            ),
            (
                "cloud.gcp",
                McpCall {
                    server: "gcp-platform".into(),
                    tool: "storage.rm".into(),
                    arguments: json!({"url": "secret-value"}),
                },
                "tool",
            ),
        ];

        for (pack, call, matched_field) in cases {
            let matched = evaluate(pack, EvaluationPayload::McpCall { call }).unwrap();
            assert_eq!(matched.pack_id, pack);
            assert_eq!(matched.matched_field, matched_field);
            assert!(!matched.rule_id.is_empty());
            assert!(!matched.reason_code.as_str().is_empty());
            assert!(!matched.description.is_empty());
            assert!(!matched.safer_action.is_empty());
            assert!(!format!("{matched:?}").contains("secret-value"));
        }

        assert!(
            evaluate(
                "cloud.aws",
                EvaluationPayload::McpCall {
                    call: McpCall {
                        server: "unrelated-service".into(),
                        tool: "resource.delete".into(),
                        arguments: json!({"action": "delete"}),
                    },
                },
            )
            .is_none()
        );
    }

    #[test]
    fn aws_mcp_pack_covers_destructive_operations_queries_and_near_misses() {
        for (tool, arguments, reason_code) in [
            (
                "ecr.batch_delete_image",
                json!({"repository_name": "app"}),
                "aws.destructive_operation",
            ),
            (
                "athena.start_query_execution",
                json!({"query_string": "DROP TABLE prod.customers"}),
                "aws.destructive_operation",
            ),
            (
                "athena.start_query_execution",
                json!({"query_string": "file:///tmp/query.sql"}),
                "aws.uninspectable_query",
            ),
            (
                "resource.execute",
                json!({"action": "kms.schedule_key_deletion"}),
                "aws.destructive_operation",
            ),
            (
                "s3.sync",
                json!({"source": "s3://a", "destination": "s3://b", "delete": true}),
                "aws.destructive_operation",
            ),
        ] {
            let matched = evaluate(
                "cloud.aws",
                EvaluationPayload::McpCall {
                    call: McpCall {
                        server: "production-aws".into(),
                        tool: tool.into(),
                        arguments,
                    },
                },
            )
            .expect("destructive AWS MCP operation");
            assert_eq!(matched.reason_code.as_str(), reason_code);
        }

        for (tool, arguments) in [
            (
                "ec2.describe_instances",
                json!({"filter": "delete-old", "action": "delete_old"}),
            ),
            (
                "athena.start_query_execution",
                json!({"query_string": "SELECT * FROM prod.customers"}),
            ),
            (
                "s3.sync",
                json!({"source": "s3://a", "destination": "s3://b", "delete": false}),
            ),
        ] {
            assert!(
                evaluate(
                    "cloud.aws",
                    EvaluationPayload::McpCall {
                        call: McpCall {
                            server: "production-aws".into(),
                            tool: tool.into(),
                            arguments,
                        },
                    },
                )
                .is_none()
            );
        }
    }

    #[test]
    fn core_packs_inspect_commands_exposed_as_mcp_tools() {
        let cases = [
            ("core.shell", "bash", json!({"command": "reboot"})),
            (
                "core.git",
                "execute_command",
                json!({"input": {"command": "git reset --hard"}}),
            ),
            (
                "core.filesystem",
                "run_command",
                json!({"cmd": "rm -rf /tmp/work"}),
            ),
        ];

        for (pack, tool, arguments) in cases {
            let matched = evaluate(
                pack,
                EvaluationPayload::McpCall {
                    call: McpCall {
                        server: "local-tools".into(),
                        tool: tool.into(),
                        arguments,
                    },
                },
            )
            .unwrap();
            assert_eq!(matched.pack_id, pack);
            assert!(matched.matched_field.starts_with("$."));
        }
    }

    #[test]
    fn malformed_generated_tool_arguments_fail_closed() {
        let error = BuiltInEvaluator
            .evaluate(
                &EvaluationInput::new(
                    GuardPhase::GeneratedToolCall,
                    EvaluationPayload::ToolCall {
                        name: "bash".into(),
                        arguments: Value::String("{not-json".into()),
                    },
                ),
                &EffectivePolicy {
                    enabled: true,
                    mode: PolicyMode::Deny,
                    packs: vec![PackId::new("core.shell").unwrap()],
                    managed_checks: Vec::new(),
                    stream_buffer_bytes: 1024,
                    stream_buffer_timeout_ms: 1_000,
                    scope: EffectiveScope::Global,
                },
            )
            .unwrap_err();
        assert_eq!(error, EvaluationError::MalformedToolCall);
    }

    #[test]
    fn registry_has_only_versioned_required_packs() {
        let metadata = PackRegistry::built_in();
        assert_eq!(
            metadata
                .iter()
                .map(|pack| pack.id.as_str())
                .collect::<BTreeSet<_>>(),
            BUILT_IN_PACK_IDS.into_iter().collect()
        );
        assert!(metadata.iter().all(|pack| pack.version == "1.0.0"));
    }

    #[test]
    fn deterministic_evaluation_load_gate_stays_below_two_seconds() {
        let started = Instant::now();
        for _ in 0..10_000 {
            let decision = evaluate(
                "core.filesystem",
                EvaluationPayload::ShellCommand {
                    command: "rm -rf /tmp/load-gate".into(),
                },
            );
            assert!(decision.is_some());
        }
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "10,000 deterministic evaluations took {:?}",
            started.elapsed()
        );
    }
}
