use crate::{MatchedRule, command::CommandInvocation};

use super::{has_option, rule, sql_statements, top_level_sql_words};

pub(super) fn match_invocation(invocation: &CommandInvocation) -> Option<MatchedRule> {
    if invocation.executable != "snow" {
        return None;
    }

    if let Some(matched) = match_direct_cli(&invocation.arguments) {
        return Some(matched);
    }

    let sql_index = invocation
        .arguments
        .iter()
        .position(|argument| argument.eq_ignore_ascii_case("sql"))?;
    let sql_arguments = &invocation.arguments[sql_index + 1..];
    if let Some(query) = option_value(sql_arguments, &["-q", "--query"]) {
        return match_sql(query);
    }
    if sql_arguments.iter().any(|argument| {
        argument == "-f"
            || argument == "--filename"
            || argument.starts_with("--filename=")
            || argument.starts_with("-f=")
    }) {
        return Some(snowflake_rule(
            "stdin-unverified",
            "snowflake.file_unverified",
            "snow sql receives SQL from a file that the command guard cannot inspect",
            "Review and submit the exact SQL inline, or use a separately protected workflow",
        ));
    }

    Some(snowflake_rule(
        "stdin-unverified",
        "snowflake.stdin_unverified",
        "snow sql receives SQL from stdin or an interactive source that cannot be inspected",
        "Materialize and review the exact SQL before execution",
    ))
}

fn match_direct_cli(arguments: &[String]) -> Option<MatchedRule> {
    let (rule_id, reason_code, description, safer_action) =
        if has_sequence(arguments, &["object", "drop", "database"])
            || has_sequence(arguments, &["object", "drop", "schema"])
        {
            (
                "cli-object-drop-database",
                "snowflake.cli_object_drop_database",
                "Drops a Snowflake database or schema and all contained objects",
                "Describe and clone the object before a reviewed drop",
            )
        } else if has_sequence(arguments, &["object", "drop"]) {
            (
                "cli-object-drop",
                "snowflake.cli_object_drop",
                "Drops a live Snowflake object",
                "Describe the exact object and review dependencies before dropping it",
            )
        } else if has_sequence(arguments, &["stage", "drop"]) {
            (
                "cli-stage-drop",
                "snowflake.cli_stage_drop",
                "Drops a stage and can permanently remove internal staged files",
                "List and preserve stage contents before dropping it",
            )
        } else if has_sequence(arguments, &["stage", "remove"]) {
            (
                "cli-stage-remove",
                "snowflake.cli_stage_remove",
                "Deletes files from a Snowflake stage",
                "List the exact stage path and preserve required files first",
            )
        } else if has_sequence(arguments, &["git", "drop"])
            || has_sequence(arguments, &["streamlit", "drop"])
        {
            (
                "cli-product-drop",
                "snowflake.cli_product_drop",
                "Drops a Snowflake Git repository or Streamlit application",
                "Preserve the definition and inspect dependencies before dropping it",
            )
        } else if has_sequence(arguments, &["dcm", "drop-deployment"])
            || has_sequence(arguments, &["dcm", "purge"])
        {
            (
                "cli-dcm-purge",
                "snowflake.cli_dcm_purge",
                "Drops deployed objects managed by a DCM project",
                "Review the deployment plan and preserve managed objects first",
            )
        } else if has_sequence(arguments, &["app", "teardown"]) {
            (
                "cli-app-teardown",
                "snowflake.cli_app_teardown",
                "Drops a Native App and can cascade to owned objects",
                "Review application-owned objects before teardown",
            )
        } else if has_sequence(arguments, &["app", "version", "drop"]) {
            (
                "cli-app-version-drop",
                "snowflake.cli_app_version_drop",
                "Removes a Native App version definition",
                "Review installed and pinned versions before removal",
            )
        } else if has_sequence(arguments, &["snowpark", "drop"]) {
            (
                "cli-snowpark-drop",
                "snowflake.cli_snowpark_drop",
                "Drops a deployed Snowpark function or procedure",
                "Inspect downstream callers before dropping the object",
            )
        } else if contains_sequence(arguments, "spcs", "drop") {
            (
                "cli-spcs-drop",
                "snowflake.cli_spcs_drop",
                "Drops Snowpark Container Services infrastructure",
                "Review dependent services and workloads before dropping it",
            )
        } else if has_sequence(arguments, &["spcs", "compute-pool", "stop-all"]) {
            (
                "cli-spcs-stop-all",
                "snowflake.cli_spcs_stop_all",
                "Deletes every service in a Snowpark Container Services compute pool",
                "Inventory services and stop them through a reviewed change",
            )
        } else if has_sequence(arguments, &["spcs", "service", "suspend"]) {
            (
                "cli-spcs-service-suspend",
                "snowflake.cli_spcs_service_suspend",
                "Shuts down every container for a Snowpark Container Services service",
                "Review service availability before suspension",
            )
        } else if has_sequence(arguments, &["dbt", "drop"]) {
            (
                "cli-dbt-drop",
                "snowflake.cli_dbt_drop",
                "Drops a Snowflake dbt project object",
                "Review scheduled and dependent dbt runs before removal",
            )
        } else if has_sequence(arguments, &["dbt", "deploy"])
            && has_option(arguments, "--force", Some('f'))
        {
            (
                "cli-dbt-deploy-force",
                "snowflake.cli_dbt_deploy_force",
                "Replaces a deployed dbt project and its run history",
                "Deploy without force after reviewing the existing project",
            )
        } else if has_sequence(arguments, &["dbt", "execute"])
            && arguments.iter().any(|argument| {
                matches!(
                    argument.as_str(),
                    "run" | "build" | "seed" | "snapshot" | "run-operation"
                )
            })
        {
            (
                "cli-dbt-execute",
                "snowflake.cli_dbt_execute",
                "Executes a dbt command that can replace or delete warehouse data",
                "Compile and review the dbt plan before execution",
            )
        } else if has_sequence(arguments, &["dcm", "drop"]) {
            (
                "cli-dcm-drop",
                "snowflake.cli_dcm_drop",
                "Drops a Snowflake DCM project object",
                "Review project deployment history before removal",
            )
        } else if (has_sequence(arguments, &["stage", "copy"])
            || has_sequence(arguments, &["dbt", "copy"]))
            && has_option(arguments, "--overwrite", None)
        {
            (
                "cli-copy-overwrite",
                "snowflake.cli_copy_overwrite",
                "Overwrites existing staged files",
                "Copy to a versioned path and inspect existing files first",
            )
        } else if (has_sequence(arguments, &["streamlit", "deploy"])
            || has_sequence(arguments, &["notebook", "deploy"]))
            && (has_option(arguments, "--replace", None) || has_option(arguments, "--prune", None))
        {
            (
                "cli-deploy-replace",
                "snowflake.cli_deploy_replace",
                "Replaces an application or deletes files absent from the deployment",
                "Preview the deployment and preserve current files first",
            )
        } else if has_sequence(arguments, &["stage", "execute"])
            || has_sequence(arguments, &["git", "execute"])
        {
            (
                "cli-uninspectable-execute",
                "snowflake.cli_uninspectable_execute",
                "Executes SQL or code from content that guardrails cannot inspect",
                "Materialize and review the exact code before execution",
            )
        } else {
            return None;
        };

    Some(snowflake_rule(
        rule_id,
        reason_code,
        description,
        safer_action,
    ))
}

fn match_sql(sql: &str) -> Option<MatchedRule> {
    sql_statements(sql)
        .into_iter()
        .find_map(|statement| match_statement(&statement))
}

fn match_statement(statement: &str) -> Option<MatchedRule> {
    let words = top_level_sql_words(statement);
    let operation_index = if words.first().is_some_and(|word| word == "with") {
        words
            .iter()
            .position(|word| matches!(word.as_str(), "delete" | "update" | "merge"))?
    } else {
        0
    };
    let words = &words[operation_index..];
    let first = words.first().map(String::as_str)?;
    let second = words.get(1).map(String::as_str);
    let third = words.get(2).map(String::as_str);
    let finding = match (first, second, third) {
        ("drop", Some("database"), _) => (
            "drop-database",
            "Drops a database and every contained object",
            "Clone and inspect the database before a reviewed drop",
        ),
        ("drop", Some("schema"), _) => (
            "drop-schema",
            "Drops a schema and can remove every contained object",
            "Clone and inspect the schema before a reviewed drop",
        ),
        ("drop", Some("table"), _) => (
            "drop-table",
            "Drops a table and its active data",
            "Clone the table and verify retention before dropping it",
        ),
        ("drop", Some(object), _) if is_data_product(object) => (
            "drop-data-product",
            "Drops a live Snowflake data product",
            "Inspect dependencies and validate a replacement first",
        ),
        ("drop", Some(object), _) if is_ingestion_object(object) => (
            "drop-ingestion-object",
            "Drops a live ingestion or scheduling object",
            "Inspect pipeline dependencies and state before removal",
        ),
        ("drop", Some("warehouse"), _) => (
            "drop-warehouse",
            "Drops compute used by applications, tasks, or users",
            "Inspect consumers and active workloads before removal",
        ),
        ("drop", Some("user" | "role"), _) => (
            "drop-principal",
            "Drops a principal and can revoke service access",
            "Review grants and disable access reversibly first",
        ),
        ("drop", Some(object), _) if is_security_object(object) => (
            "drop-security-object",
            "Drops a live security or sharing object",
            "Review consumers and grants before removal",
        ),
        ("drop", _, _) => (
            "drop-programmable-object",
            "Drops a live Snowflake object",
            "Preserve the definition and inspect dependencies before removal",
        ),
        ("truncate", _, _) => (
            "truncate-table",
            "Removes every row from a table",
            "Preview the row count and clone the table before truncation",
        ),
        ("delete", _, _) if !words.iter().any(|word| word == "where") => (
            "delete-all",
            "Deletes every row from the target table",
            "Add a reviewed WHERE predicate and preview matching rows",
        ),
        ("delete", _, _) => (
            "bounded-delete",
            "Deletes every row selected by the WHERE predicate",
            "Preview matching rows against a clone before deletion",
        ),
        ("update", _, _) if !words.iter().any(|word| word == "where") => (
            "update-all",
            "Updates every row in the target table",
            "Add a reviewed WHERE predicate and preview matching rows",
        ),
        ("update", _, _) => (
            "bounded-update",
            "Updates every row selected by the WHERE predicate",
            "Preview matching rows against a clone before the update",
        ),
        ("create", Some("or"), Some("replace")) => (
            replace_rule(words),
            "Replaces a live Snowflake object",
            "Create and validate a separate object before cutover",
        ),
        ("remove", _, _) => (
            "remove-stage-files",
            "Deletes files from an internal Snowflake stage",
            "List the exact stage path and preserve required files first",
        ),
        ("alter", Some("pipe"), _) if words.iter().any(|word| word == "pause_pipe") => (
            "pause-pipe",
            "Pauses ingestion and can make downstream data stale",
            "Inspect pipe status and freshness requirements first",
        ),
        ("alter", Some("task"), _) if words.iter().any(|word| word == "suspend") => (
            "suspend-task",
            "Stops scheduled task execution",
            "Inspect task dependencies and active runs first",
        ),
        ("execute", Some("task"), _) => (
            "execute-task",
            "Immediately starts a task and may cascade through its graph",
            "Inspect the task graph and active runs before execution",
        ),
        ("alter", Some("warehouse"), _) if words.iter().any(|word| word == "suspend") => (
            "suspend-warehouse",
            "Can interrupt active or queued warehouse workloads",
            "Inspect active queries and dependent tasks first",
        ),
        ("alter", Some("warehouse"), _) if words.iter().any(|word| word == "set") => (
            "warehouse-settings",
            "Changes warehouse availability or cost settings",
            "Review consumers, size, scaling, and auto-suspend settings",
        ),
        ("revoke", _, _) => (
            "broad-revoke",
            "Revokes privileges or role membership",
            "Review every affected principal before revoking access",
        ),
        ("grant", Some("ownership"), _) => (
            "transfer-ownership",
            "Transfers object control and can change current grants",
            "Review outbound privileges and current-grant semantics first",
        ),
        ("grant", _, _) if broad_grant(words) => (
            "broad-grant",
            "Creates broad or account-level access",
            "Grant only required privileges to a least-privilege role",
        ),
        ("alter", Some("table"), Some("drop")) => (
            "alter-table-drop-column",
            "Removes a table column and its active data",
            "Clone the table and validate downstream consumers first",
        ),
        ("alter", Some("table"), _) if words.iter().any(|word| word == "swap") => (
            "alter-table-swap",
            "Atomically exchanges table identities",
            "Compare both tables and verify fully qualified names first",
        ),
        ("alter", Some("table"), Some("rename")) => (
            "rename-object",
            "Renames a table and can break qualified consumers",
            "Inventory consumers and coordinate a reviewed cutover",
        ),
        ("alter", Some("table"), Some("alter" | "modify")) => (
            "alter-column",
            "Changes a column contract and can break consumers",
            "Validate type and constraint changes against a clone",
        ),
        ("insert", Some("overwrite"), _) => (
            "insert-overwrite",
            "Replaces the target table's current rows",
            "Write to and validate a separate table first",
        ),
        ("copy", Some("into"), _) if words.iter().any(|word| word == "overwrite") => (
            "copy-overwrite",
            "Can replace exported files",
            "Export to a versioned path and list the destination first",
        ),
        ("copy", Some("into"), _) => (
            "copy-into-table",
            "Can load duplicate or unexpected data into a table",
            "Validate staged files and load into a clone first",
        ),
        ("put", _, _) if words.iter().any(|word| word == "overwrite") => (
            "put-overwrite",
            "Can replace files in an internal stage",
            "Upload to a versioned path and list existing files first",
        ),
        ("merge", _, _) => (
            "merge-data",
            "Can update, insert, or delete rows based on source matches",
            "Validate source uniqueness and preview matches against a clone",
        ),
        ("execute", Some("immediate"), _) => (
            "execute-immediate",
            "Runs generated SQL whose rendered semantics are not visible",
            "Materialize and review the exact rendered SQL first",
        ),
        ("alter", _, _) if snowflake_alter_removes_state(words) => (
            "alter-remove-state",
            "Removes a Snowflake version, specification, key, or access token",
            "Preserve the current configuration and review every dependent consumer",
        ),
        _ if first == "!abort" => (
            "abort-query",
            "Cancels an active Snowflake query",
            "Inspect query history and confirm the exact query ID",
        ),
        _ if first == "!edit" => (
            "interactive-edit",
            "Executes SQL modified in an external editor",
            "Materialize the final SQL in a reviewed local file",
        ),
        _ => return None,
    };

    Some(snowflake_rule(
        finding.0,
        &format!("snowflake.{}", finding.0.replace('-', "_")),
        finding.1,
        finding.2,
    ))
}

fn replace_rule(words: &[String]) -> &'static str {
    match words.get(3).map(String::as_str) {
        Some("database") => "replace-database",
        Some("schema") => "replace-schema",
        Some("table") => "replace-table",
        _ => "replace-live-object",
    }
}

fn is_data_product(object: &str) -> bool {
    matches!(object, "view" | "materialized" | "dynamic")
}

fn is_ingestion_object(object: &str) -> bool {
    matches!(object, "stage" | "pipe" | "stream" | "task")
}

fn is_security_object(object: &str) -> bool {
    matches!(object, "integration" | "network" | "share")
}

fn snowflake_alter_removes_state(words: &[String]) -> bool {
    [
        &["drop", "configuration"][..],
        &["drop", "specification"][..],
        &["drop", "version"][..],
        &["remove", "programmatic", "access", "token"][..],
        &["remove", "key", "pair"][..],
    ]
    .iter()
    .any(|sequence| has_word_sequence(words, sequence))
}

fn has_word_sequence(words: &[String], sequence: &[&str]) -> bool {
    words.windows(sequence.len()).any(|window| {
        window
            .iter()
            .map(String::as_str)
            .eq(sequence.iter().copied())
    })
}

fn broad_grant(words: &[String]) -> bool {
    words
        .iter()
        .any(|word| matches!(word.as_str(), "ownership" | "accountadmin"))
        || words
            .windows(2)
            .any(|window| window == ["all", "privileges"])
}

fn has_sequence(arguments: &[String], sequence: &[&str]) -> bool {
    arguments.windows(sequence.len()).any(|window| {
        window
            .iter()
            .map(String::as_str)
            .eq(sequence.iter().copied())
    })
}

fn contains_sequence(arguments: &[String], first: &str, last: &str) -> bool {
    arguments.iter().enumerate().any(|(index, argument)| {
        argument == first
            && arguments[index + 1..]
                .iter()
                .take(3)
                .any(|candidate| candidate == last)
    })
}

fn option_value<'a>(arguments: &'a [String], names: &[&str]) -> Option<&'a str> {
    arguments.iter().enumerate().find_map(|(index, argument)| {
        names.iter().find_map(|name| {
            if argument == name {
                arguments.get(index + 1).map(String::as_str)
            } else {
                argument
                    .strip_prefix(name)
                    .and_then(|value| value.strip_prefix('='))
            }
        })
    })
}

fn snowflake_rule(
    rule_id: &str,
    reason_code: &str,
    description: &str,
    safer_action: &str,
) -> MatchedRule {
    rule(
        "database.snowflake",
        rule_id,
        "command.arguments",
        reason_code,
        description,
        safer_action,
    )
}
