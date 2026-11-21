pub fn newer_migration_sql(version: u32, minimum: Option<u32>) -> String {
    let mut sql = format!(
        "INSERT INTO refinery_schema_history (version, name, applied_on, checksum) VALUES ({version}, 'future_{version}', 0, 'future-checksum');"
    );
    if let Some(minimum) = minimum {
        sql.push_str(&format!(
            "INSERT INTO gateway_migration_compatibility (version, min_reader_version) VALUES ({version}, {minimum});"
        ));
    }
    sql
}
