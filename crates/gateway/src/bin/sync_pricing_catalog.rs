use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::Context;
use gateway_service::{
    CatalogSourceId, fetch_vendored_source_snapshot, snapshot_to_pretty_json,
    vendored_snapshot_is_unchanged,
};

const MODELS_DEV_OUTPUT_PATH: &str = "crates/gateway-service/data/pricing_catalog_fallback.json";
const LITELLM_OUTPUT_PATH: &str = "crates/gateway-service/data/litellm_catalog_fallback.json";
const OPENROUTER_OUTPUT_PATH: &str = "crates/gateway-service/data/openrouter_catalog_fallback.json";

/// Refresh every vendored catalog fallback. The optional first argument overrides the
/// models.dev output path.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let models_dev_path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(MODELS_DEV_OUTPUT_PATH));
    let targets = [
        (CatalogSourceId::ModelsDev, models_dev_path),
        (CatalogSourceId::LiteLlm, PathBuf::from(LITELLM_OUTPUT_PATH)),
        (
            CatalogSourceId::OpenRouter,
            PathBuf::from(OPENROUTER_OUTPUT_PATH),
        ),
    ];
    for (source, output_path) in targets {
        sync_source(source, &output_path).await?;
    }
    Ok(())
}

async fn sync_source(source: CatalogSourceId, output_path: &Path) -> anyhow::Result<()> {
    let snapshot = fetch_vendored_source_snapshot(source)
        .await
        .with_context(|| format!("failed refreshing vendored {source} catalog"))?;

    // A missing or unreadable file is rewritten below, which reports any real I/O error.
    if let Ok(existing) = fs::read_to_string(output_path)
        && vendored_snapshot_is_unchanged(&existing, &snapshot)
    {
        println!(
            "{source} catalog fallback at {} is unchanged",
            output_path.display()
        );
        return Ok(());
    }

    let json = snapshot_to_pretty_json(&snapshot)?;
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed creating catalog output directory `{}`",
                parent.display()
            )
        })?;
    }
    fs::write(output_path, json).with_context(|| {
        format!(
            "failed writing vendored {source} catalog to `{}`",
            output_path.display()
        )
    })?;

    println!(
        "wrote {source} catalog fallback to {}",
        output_path.display()
    );
    Ok(())
}
