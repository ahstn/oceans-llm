use std::{env, path::PathBuf};

use gateway::http::admin_contract::{ADMIN_OPENAPI_PATH, write_admin_openapi};

fn main() -> anyhow::Result<()> {
    let output = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(ADMIN_OPENAPI_PATH));

    write_admin_openapi(&output)
}
