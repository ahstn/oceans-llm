use std::{env, fs, path::PathBuf};

use anyhow::Context;
use gateway_service::{
    DEFAULT_PRICING_CATALOG_SOURCE_URL, fetch_vendored_snapshot, snapshot_to_pretty_json,
    vendored_snapshot_is_unchanged,
};

const DEFAULT_OUTPUT_PATH: &str = "crates/gateway-service/data/pricing_catalog_fallback.json";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let output_path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_OUTPUT_PATH));

    let snapshot = fetch_vendored_snapshot(DEFAULT_PRICING_CATALOG_SOURCE_URL)
        .await
        .with_context(|| {
            format!(
                "failed refreshing vendored pricing catalog from `{DEFAULT_PRICING_CATALOG_SOURCE_URL}`"
            )
        })?;

    // A missing or unreadable file is rewritten below, which reports any real I/O error.
    if let Ok(existing) = fs::read_to_string(&output_path)
        && vendored_snapshot_is_unchanged(&existing, &snapshot)
    {
        println!(
            "pricing catalog fallback at {} is unchanged",
            output_path.display()
        );
        return Ok(());
    }

    let json = snapshot_to_pretty_json(&snapshot)?;
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed creating pricing catalog output directory `{}`",
                parent.display()
            )
        })?;
    }
    fs::write(&output_path, json).with_context(|| {
        format!(
            "failed writing vendored pricing catalog to `{}`",
            output_path.display()
        )
    })?;

    println!(
        "wrote pricing catalog fallback to {}",
        output_path.display()
    );
    Ok(())
}
