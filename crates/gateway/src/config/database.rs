use super::*;

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default = "default_postgres_max_connections")]
    pub max_connections: u32,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            kind: Some(default_db_kind()),
            path: Some(default_db_path()),
            url: None,
            max_connections: default_postgres_max_connections(),
        }
    }
}

impl DatabaseConfig {
    pub fn connection_options(&self) -> anyhow::Result<StoreConnectionOptions> {
        let kind = self.kind.as_deref().unwrap_or_else(|| {
            if self.url.is_some() {
                "postgres"
            } else {
                "libsql"
            }
        });

        match kind {
            "libsql" => {
                let path = self.path.as_ref().cloned().unwrap_or_else(default_db_path);
                Ok(StoreConnectionOptions::Libsql { path: path.into() })
            }
            "postgres" => {
                let raw_url = self.url.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("database.url is required when database.kind=postgres")
                })?;
                let url = resolve_secret_reference(raw_url)?;
                Ok(StoreConnectionOptions::Postgres {
                    url,
                    max_connections: self.max_connections,
                })
            }
            other => bail!("unsupported database.kind `{other}`; use libsql or postgres"),
        }
    }
}

fn default_db_path() -> String {
    "./gateway.db".to_string()
}

fn default_db_kind() -> String {
    "libsql".to_string()
}

const fn default_postgres_max_connections() -> u32 {
    10
}
