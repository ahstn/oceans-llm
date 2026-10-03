use std::{fs::File, io::Read, path::Path};

use anyhow::{Context, bail};
use gateway_skills::{
    BundleLimits, ClaimNamespaceRequest, SetDefaultVersionRequest, SkillDetail, SkillNamespace,
    SkillSummary, SkillUploadResponse, SkillVersionDetail, SkillVersionSummary, ValidatedBundle,
    inspect_archive, pack_directory_excluding, validate_name, validate_namespace,
};
use serde::Serialize;
use serde_json::json;

use crate::{
    cli::{SkillsCommand, VersionArgs},
    client::Client,
    install::{self, InstallRecord},
};

pub async fn run(client: &Client, command: SkillsCommand, as_json: bool) -> anyhow::Result<()> {
    match command {
        SkillsCommand::Namespace { handle } => namespace(client, handle, as_json).await,
        SkillsCommand::List { namespace } => list(client, namespace, as_json).await,
        SkillsCommand::Show(selected) => show(client, &selected, as_json).await,
        SkillsCommand::Upload { path } => upload(client, &path, as_json).await,
        SkillsCommand::Versions { skill } => versions(client, &skill, as_json).await,
        SkillsCommand::SetDefault { skill, version } => {
            let detail = resolve(client, &skill).await?;
            let updated: SkillDetail = client
                .put(
                    &[&detail.skill.id.to_string(), "default-version"],
                    &SetDefaultVersionRequest { version },
                )
                .await?;
            emit(
                &updated,
                &format!("{skill}: default version {}", updated.skill.default_version),
                as_json,
            )
        }
        SkillsCommand::Download { selected, output } => {
            let fetched = fetch_archive(client, &selected).await?;
            install::save_download(&fetched.bytes, &output)?;
            emit(
                &json!({"path": output, "skill_id": fetched.skill.id, "version": fetched.version.version,
                "sha256": fetched.version.sha256}),
                &format!("Saved {}", output.display()),
                as_json,
            )
        }
        SkillsCommand::Install {
            selected,
            directory,
            replace,
        } => {
            let fetched = fetch_archive(client, &selected).await?;
            let record = InstallRecord {
                namespace: fetched.skill.namespace,
                skill_id: fetched.skill.id,
                version: fetched.version.version,
                sha256: fetched.version.sha256,
            };
            let path = install::install_bundle(&fetched.bundle, &record, &directory, replace)?;
            emit(
                &json!({"path": path, "installation": record}),
                &format!("Installed {}", path.display()),
                as_json,
            )
        }
    }
}

async fn namespace(client: &Client, handle: Option<String>, as_json: bool) -> anyhow::Result<()> {
    let namespace: Option<SkillNamespace> = match handle {
        Some(handle) => {
            validate_namespace(&handle)?;
            Some(
                client
                    .post(&["namespace"], &ClaimNamespaceRequest { handle })
                    .await?,
            )
        }
        None => client.get(&["namespace"]).await?,
    };
    let display = namespace
        .as_ref()
        .map(|value| value.handle.as_str())
        .unwrap_or("No user namespace. Register one with: oceans skills namespace <handle>");
    emit(&namespace, display, as_json)
}

async fn list(client: &Client, namespace: Option<String>, as_json: bool) -> anyhow::Result<()> {
    if let Some(namespace) = &namespace {
        validate_namespace(namespace)?;
    }
    let mut skills = Vec::<SkillSummary>::new();
    loop {
        let offset = skills.len().to_string();
        let mut query = vec![("limit", "100"), ("offset", offset.as_str())];
        if let Some(namespace) = &namespace {
            query.push(("namespace", namespace));
        }
        let page: Vec<SkillSummary> = client.get_query(&[], &query).await?;
        let has_more = page.len() == 100;
        if page.len() > 100 {
            bail!("gateway returned more skills than the requested page size");
        }
        skills.extend(page);
        if !has_more {
            break;
        }
    }
    let display = skills
        .iter()
        .map(|skill| {
            format!(
                "{}/{}\tdefault {}\tlatest {}\t{}",
                skill.namespace,
                skill.name,
                skill.default_version,
                skill.latest_version,
                skill.description
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    emit(&skills, &display, as_json)
}

async fn show(client: &Client, selected: &VersionArgs, as_json: bool) -> anyhow::Result<()> {
    let detail = resolve(client, &selected.skill).await?;
    let version = selected.version.unwrap_or(detail.skill.default_version);
    // JSON can escape each input byte as six ASCII bytes. Only previews need this larger bound.
    let limits: BundleLimits = client.get(&["limits"]).await?;
    let preview_limit = usize::try_from(limits.max_expanded_bytes)?
        .checked_mul(6)
        .and_then(|limit| limit.checked_add(4 * 1024 * 1024))
        .context("configured bundle limit is too large for this client")?;
    let selected_version: SkillVersionDetail = client
        .get_with_limit(
            &[
                &detail.skill.id.to_string(),
                "versions",
                &version.to_string(),
            ],
            preview_limit,
        )
        .await?;
    emit(
        &json!({"skill": detail.skill, "version": selected_version}),
        &format!(
            "{} version {}\n\n{}",
            selected.skill, version, selected_version.instructions
        ),
        as_json,
    )
}

async fn versions(client: &Client, skill: &str, as_json: bool) -> anyhow::Result<()> {
    let detail = resolve(client, skill).await?;
    let versions: Vec<SkillVersionSummary> = client
        .get(&[&detail.skill.id.to_string(), "versions"])
        .await?;
    let display = versions
        .iter()
        .map(|version| {
            format!(
                "{}{}\t{} bytes\t{}",
                version.version,
                if version.version == detail.skill.default_version {
                    " (default)"
                } else {
                    ""
                },
                version.archive_bytes,
                version.sha256
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    emit(&versions, &display, as_json)
}

async fn upload(client: &Client, path: &Path, as_json: bool) -> anyhow::Result<()> {
    let namespace: Option<SkillNamespace> = client.get(&["namespace"]).await?;
    let namespace =
        namespace.context("register a user namespace first: oceans skills namespace <handle>")?;
    let limits: BundleLimits = client.get(&["limits"]).await?;
    let bundle = read_upload(path, &limits)?;
    let existing: Option<SkillDetail> = client
        .get_optional(&["by-name", &namespace.handle, &bundle.manifest.name])
        .await?;
    let result: SkillUploadResponse = match existing {
        Some(existing) => {
            client
                .upload(
                    &[&existing.skill.id.to_string(), "versions"],
                    bundle.archive,
                )
                .await?
        }
        None => client.upload(&[], bundle.archive).await?,
    };
    emit(
        &result,
        &format!(
            "Uploaded {}/{} version {} (default {})",
            result.detail.skill.namespace,
            result.detail.skill.name,
            result.uploaded_version,
            result.detail.skill.default_version
        ),
        as_json,
    )
}

fn read_upload(path: &Path, limits: &BundleLimits) -> anyhow::Result<ValidatedBundle> {
    if path.is_dir() {
        let excluded = if install::read_install_record(path)?.is_some() {
            &[install::INSTALL_RECORD][..]
        } else {
            &[]
        };
        let bundle = pack_directory_excluding(path, limits, excluded)
            .context("could not package skill directory")?;
        install::ensure_no_installer_record(&bundle)?;
        return Ok(bundle);
    }
    let file = File::open(path).with_context(|| format!("could not open {}", path.display()))?;
    if !file.metadata()?.is_file() {
        bail!("upload must be a skill directory or a regular ZIP file");
    }
    let mut bytes = Vec::new();
    file.take(limits.max_archive_bytes.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limits.max_archive_bytes {
        bail!("skill archive exceeds the upload size limit");
    }
    let bundle = inspect_archive(&bytes, limits).context("skill archive is invalid")?;
    install::ensure_no_installer_record(&bundle)?;
    Ok(bundle)
}

async fn resolve(client: &Client, address: &str) -> anyhow::Result<SkillDetail> {
    let (namespace, name) = parse_address(address)?;
    client.get(&["by-name", namespace, name]).await
}

fn parse_address(address: &str) -> anyhow::Result<(&str, &str)> {
    let (namespace, name) = address
        .split_once('/')
        .context("use a skill address in namespace/skill-name form")?;
    validate_namespace(namespace)?;
    validate_name(name)?;
    Ok((namespace, name))
}

struct FetchedArchive {
    skill: SkillSummary,
    version: SkillVersionSummary,
    bytes: Vec<u8>,
    bundle: ValidatedBundle,
}

async fn fetch_archive(client: &Client, selected: &VersionArgs) -> anyhow::Result<FetchedArchive> {
    let detail = resolve(client, &selected.skill).await?;
    let version = selected.version.unwrap_or(detail.skill.default_version);
    let id = detail.skill.id.to_string();
    let version_path = version.to_string();
    let metadata = detail
        .versions
        .into_iter()
        .find(|candidate| candidate.version == version)
        .with_context(|| format!("skill has no version {version}"))?;
    let limits: BundleLimits = client.get(&["limits"]).await?;
    let limit = usize::try_from(limits.max_archive_bytes)?;
    let bytes = client
        .download(&[&id, "versions", &version_path, "archive"], limit)
        .await?;
    if bytes.len() as u64 != metadata.archive_bytes {
        bail!("archive size does not match version metadata");
    }
    let bundle = install::verify_archive(&bytes, &metadata.sha256, &limits)?;
    if bundle.manifest.name != detail.skill.name {
        bail!("archive name does not match the requested skill");
    }
    Ok(FetchedArchive {
        skill: detail.skill,
        version: metadata,
        bytes,
        bundle,
    })
}

fn emit(value: &impl Serialize, display: &str, as_json: bool) -> anyhow::Result<()> {
    if as_json {
        println!("{}", serde_json::to_string_pretty(value)?);
    } else if !display.is_empty() {
        // Skill descriptions and instructions are untrusted terminal text.
        let safe: String = display
            .chars()
            .filter(|value| !value.is_control() || matches!(value, '\n' | '\t'))
            .collect();
        println!("{safe}");
    }
    Ok(())
}

#[cfg(test)]
#[path = "skills_tests.rs"]
mod workflow_tests;

#[cfg(test)]
mod tests {
    use super::parse_address;

    #[test]
    fn skill_addresses_are_not_arbitrary_paths() {
        assert_eq!(parse_address("alice/review").unwrap(), ("alice", "review"));
        for invalid in [
            "review",
            "alice/../review",
            "alice/review/extra",
            "alice/",
            "/review",
        ] {
            assert!(parse_address(invalid).is_err(), "{invalid}");
        }
    }
}
