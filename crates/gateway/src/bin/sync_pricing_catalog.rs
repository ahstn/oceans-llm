use std::{env, path::PathBuf};

use anyhow::Context;
use gateway_service::{
    DEFAULT_PRICING_CATALOG_SOURCE_URL, fetch_vendored_snapshot, snapshot_to_pretty_json,
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
    let json = snapshot_to_pretty_json(&snapshot)?;

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed creating pricing catalog output directory `{}`",
                parent.display()
            )
        })?;
    }
    std::fs::write(&output_path, json).with_context(|| {
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
