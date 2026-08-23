use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::{
    DeterministicEvaluator, EffectivePolicy, EvaluationError, EvaluationInput, EvaluationPayload,
    MatchedRule, ReasonCode,
    command::{CommandInvocation, parse_command_line},
    selectors::{JsonPath, McpCall},
};

pub const BUILT_IN_PACK_IDS: [&str; 7] = [
    "core.shell",
    "core.git",
    "core.filesystem",
    "database.postgresql",
    "cloud.aws",
    "cloud.gcp",
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
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-')
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
            metadata("cloud.aws", "Destructive AWS CLI and MCP operations"),
            metadata(
                "cloud.gcp",
                "Destructive Google Cloud CLI and MCP operations",
            ),
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

fn match_tool_call_pack(pack: &str, name: &str, arguments: &Value) -> Option<MatchedRule> {
    if is_shell_tool(name) {
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
    let invocations = parse_command_line(command);
    for invocation in &invocations {
        let finding = match pack {
            "core.shell" => match_core_shell(invocation),
            "core.git" => match_git(invocation),
            "core.filesystem" => match_filesystem(invocation),
            "database.postgresql" => match_postgresql_invocation(invocation),
            "cloud.aws" => match_aws(invocation),
            "cloud.gcp" => match_gcp(invocation),
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
        _ => None,
    }
}

fn targets_pid_one(arguments: &[String]) -> bool {
    arguments
        .iter()
        .filter(|argument| !argument.starts_with('-'))
        .any(|argument| argument == "1")
}

fn match_git(invocation: &CommandInvocation) -> Option<MatchedRule> {
    if invocation.executable != "git" {
        return None;
    }
    let arguments = &invocation.arguments;
    let operation = git_operation(arguments)?;
    match operation {
        "reset" if has_option(arguments, "--hard", None) => Some(rule(
            "core.git",
            "reset-hard",
            "command.arguments",
            "git.reset_hard",
            "Discards tracked working-tree changes",
            "Create a backup branch or use git stash before resetting",
        )),
        "clean" if has_option(arguments, "--force", Some('f')) => Some(rule(
            "core.git",
            "clean-force",
            "command.arguments",
            "git.clean_force",
            "Deletes untracked files",
            "Run git clean --dry-run and remove reviewed paths explicitly",
        )),
        "push"
            if has_option(arguments, "--force", Some('f'))
                || has_option(arguments, "--force-with-lease", None) =>
        {
            Some(rule(
                "core.git",
                "push-force",
                "command.arguments",
                "git.push_force",
                "Rewrites remote branch history",
                "Use a normal push or coordinate a reviewed force-with-lease operation",
            ))
        }
        "branch" if arguments.iter().any(|argument| argument == "-D") => Some(rule(
            "core.git",
            "branch-delete-force",
            "command.arguments",
            "git.branch_delete_force",
            "Deletes a branch without merge checks",
            "Use git branch -d after the branch is merged",
        )),
        _ => None,
    }
}

fn match_filesystem(invocation: &CommandInvocation) -> Option<MatchedRule> {
    match invocation.executable.as_str() {
        "rm" if (has_option(&invocation.arguments, "--recursive", Some('r'))
            || has_option(&invocation.arguments, "--recursive", Some('R')))
            && has_option(&invocation.arguments, "--force", Some('f')) =>
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
        "find"
            if invocation
                .arguments
                .iter()
                .any(|argument| argument == "-delete") =>
        {
            Some(rule(
                "core.filesystem",
                "find-delete",
                "command.arguments",
                "filesystem.find_delete",
                "Deletes every path selected by find",
                "Run find without -delete and review the selected paths",
            ))
        }
        _ => None,
    }
}

fn match_postgresql_invocation(invocation: &CommandInvocation) -> Option<MatchedRule> {
    if !matches!(invocation.executable.as_str(), "psql" | "pgcli") {
        return None;
    }
    let sql = option_value(&invocation.arguments, &["-c", "--command"])?;
    match_destructive_sql(sql, "command.arguments")
}

fn match_destructive_sql(sql: &str, field: &str) -> Option<MatchedRule> {
    sql_statements(sql)
        .into_iter()
        .find_map(|statement| match_destructive_sql_statement(&statement, field))
}

fn match_destructive_sql_statement(sql: &str, field: &str) -> Option<MatchedRule> {
    let normalized = sql
        .split_whitespace()
        .map(|word| word.trim_matches(|character: char| matches!(character, ';' | '"')))
        .collect::<Vec<_>>();
    let first = normalized.first()?.to_ascii_lowercase();
    let second = normalized.get(1).map(|word| word.to_ascii_lowercase());
    match (first.as_str(), second.as_deref()) {
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
        ("truncate", _) => Some(rule(
            "database.postgresql",
            "truncate",
            field,
            "postgresql.truncate",
            "Removes all rows from a table",
            "Use a scoped DELETE in a transaction after a verified backup",
        )),
        ("delete", Some("from"))
            if !normalized
                .iter()
                .any(|word| word.eq_ignore_ascii_case("where")) =>
        {
            Some(rule(
                "database.postgresql",
                "delete-without-where",
                field,
                "postgresql.delete_without_where",
                "Deletes every row from a table",
                "Add a reviewed WHERE clause and run inside a transaction",
            ))
        }
        _ => None,
    }
}

fn match_aws(invocation: &CommandInvocation) -> Option<MatchedRule> {
    if invocation.executable != "aws" {
        return None;
    }
    let destructive = invocation.arguments.iter().any(|argument| {
        argument.starts_with("delete-")
            || argument.starts_with("terminate-")
            || argument.starts_with("deregister-")
    }) || (invocation.arguments.iter().any(|argument| argument == "s3")
        && invocation.arguments.iter().any(|argument| argument == "rm")
        && has_option(&invocation.arguments, "--recursive", None));
    destructive.then(|| {
        rule(
            "cloud.aws",
            "destructive-operation",
            "command.arguments",
            "aws.destructive_operation",
            "Runs a destructive AWS operation",
            "Use a read-only describe command and follow the approved cloud change process",
        )
    })
}

fn match_gcp(invocation: &CommandInvocation) -> Option<MatchedRule> {
    if invocation.executable != "gcloud" {
        return None;
    }
    positional_arguments(&invocation.arguments)
        .iter()
        .any(|argument| argument == "delete")
        .then(|| {
            rule(
                "cloud.gcp",
                "delete-resource",
                "command.arguments",
                "gcp.delete_resource",
                "Deletes a Google Cloud resource",
                "Describe the resource and follow the approved cloud change process",
            )
        })
}

fn match_mcp_pack(pack: &str, call: &McpCall) -> Option<MatchedRule> {
    match pack {
        "database.postgresql" => match_postgresql_mcp(call),
        "cloud.aws" => match_operation_mcp(
            call,
            &["aws", "amazon"],
            &["delete", "terminate", "destroy", "remove", "deregister"],
            "cloud.aws",
            "destructive-operation",
            "aws.destructive_operation",
            "Runs a destructive AWS operation",
        ),
        "cloud.gcp" => match_operation_mcp(
            call,
            &["gcp", "google_cloud", "google"],
            &["delete", "destroy", "remove"],
            "cloud.gcp",
            "delete-resource",
            "gcp.delete_resource",
            "Deletes a Google Cloud resource",
        ),
        "saas.notion" => match_notion(call),
        _ => None,
    }
}

fn match_postgresql_mcp(call: &McpCall) -> Option<MatchedRule> {
    if !server_has_identity(&call.server, &["postgres", "postgresql", "pg"]) {
        return None;
    }
    let tool = normalize_identity(&call.tool);
    if !matches!(
        tool.as_str(),
        "execute_sql" | "run_sql" | "query" | "postgres.execute" | "postgresql.execute"
    ) {
        return None;
    }
    for path in ["$.sql", "$.query", "$.statement", "$.input.sql"] {
        if let Some(sql) = first_string(&call.arguments, path)
            && let Some(matched) = match_destructive_sql(sql, path)
        {
            return Some(matched);
        }
    }
    None
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
    );
    if update_alias {
        for path in ["$.archived", "$.in_trash", "$.deleted"] {
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
    arguments.iter().any(|argument| {
        argument == long
            || argument.strip_prefix(&format!("{long}=")).is_some()
            || short.is_some_and(|short| {
                argument.starts_with('-')
                    && !argument.starts_with("--")
                    && argument[1..].chars().any(|candidate| candidate == short)
            })
    })
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
                vec!["reboot", "bash -c 'poweroff'", "kill 1 -9"],
                vec!["echo 'reboot'", "reboot-check", "kill 10 -9"],
            ),
            (
                "core.git",
                vec![
                    "git reset --hard",
                    "bash -c 'git branch -D old'",
                    "git --no-pager reset main --hard",
                    "git -C /repo reset --hard",
                    "git --git-dir=/repo/.git reset --hard",
                ],
                vec![
                    "printf '%s' 'git reset --hard'",
                    "git reset --soft HEAD~1",
                    "git branch -d merged",
                ],
            ),
            (
                "core.filesystem",
                vec![
                    "rm -rf /tmp/work",
                    "bash -c 'find . -delete'",
                    "rm --force --recursive /tmp/work",
                    "rm -Rf /tmp/work",
                    "rm -R -f /tmp/work",
                ],
                vec![
                    "echo 'rm -rf /tmp/work'",
                    "rm -f report.txt",
                    "find . -depth",
                ],
            ),
            (
                "database.postgresql",
                vec![
                    "psql -c 'DROP DATABASE app'",
                    "bash -c \"psql -c 'TRUNCATE audit'\"",
                    "psql --host=db --command='DELETE FROM users'",
                    "psql -c 'SELECT 1; DROP DATABASE app'",
                ],
                vec![
                    "echo 'DROP DATABASE app'",
                    "psql -c 'DELETE FROM users WHERE id = 1'",
                    "psql -c 'SELECT drop_database_hint FROM docs'",
                ],
            ),
            (
                "cloud.aws",
                vec![
                    "aws ec2 terminate-instances --instance-ids i-1",
                    "bash -c 'aws s3 rm s3://bucket --recursive'",
                    "aws --region us-east-1 ec2 delete-vpc --vpc-id vpc-1",
                ],
                vec![
                    "echo 'aws ec2 terminate-instances'",
                    "aws ec2 describe-instances",
                    "aws s3 ls s3://bucket",
                ],
            ),
            (
                "cloud.gcp",
                vec![
                    "gcloud projects delete example",
                    "bash -c 'gcloud compute instances delete vm'",
                    "gcloud --quiet compute disks delete disk",
                ],
                vec![
                    "echo 'gcloud projects delete example'",
                    "gcloud projects describe example",
                    "gcloud compute instances list",
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
