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
                "snow sql -q 'ALTER TABLE users DROP PRIMARY KEY'",
                "snow sql -q 'ALTER TABLE users DROP UNIQUE (email)'",
                "snow sql -q 'ALTER TABLE users DROP FOREIGN KEY (role_id)'",
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
                "kubectl --as-uid 1000 delete pod api",
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
                "helm --kube-tls-server-name api.internal uninstall release",
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
