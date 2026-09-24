use std::{env, path::PathBuf};

use anyhow::Context;
use gateway_service::{
    BenchmarkSnapshot, DEFAULT_BENCHMARK_SOURCE_URL, benchmark_snapshot_to_pretty_json,
    empty_benchmark_snapshot, fetch_openrouter_benchmark_models, merge_benchmark_models,
};
use time::OffsetDateTime;

const DEFAULT_OUTPUT_PATH: &str = "crates/gateway-service/data/model_benchmarks.json";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let output_path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_OUTPUT_PATH));
    // Whole seconds keep the vendored JSON diff readable.
    let now = OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .expect("zero nanoseconds is valid");

    let mut snapshot = match std::fs::read_to_string(&output_path) {
        Ok(existing) => serde_json::from_str::<BenchmarkSnapshot>(&existing).with_context(|| {
            format!(
                "existing benchmark snapshot `{}` is invalid",
                output_path.display()
            )
        })?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            empty_benchmark_snapshot(DEFAULT_BENCHMARK_SOURCE_URL, now)
        }
        Err(error) => {
            return Err(error).with_context(|| {
                format!("failed reading benchmark snapshot `{}`", output_path.display())
            });
        }
    };

    let fetched = fetch_openrouter_benchmark_models(DEFAULT_BENCHMARK_SOURCE_URL).await?;
    let fetched_count = fetched.len();
    if !merge_benchmark_models(&mut snapshot, fetched, DEFAULT_BENCHMARK_SOURCE_URL, now)
        && output_path.exists()
    {
        println!("benchmark snapshot unchanged ({fetched_count} models fetched)");
        return Ok(());
    }

    std::fs::write(&output_path, benchmark_snapshot_to_pretty_json(&snapshot)?).with_context(
        || format!("failed writing benchmark snapshot to `{}`", output_path.display()),
    )?;
    println!(
        "wrote {} models ({fetched_count} fetched) to {}",
        snapshot.models.len(),
        output_path.display()
    );
    Ok(())
}
