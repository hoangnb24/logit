use logit::sqlite::{
    EVENT_INSERT_COLUMNS, SCHEMA_META_TABLE, SQLITE_SCHEMA_VERSION, create_schema_sql,
    schema_statements,
};

#[test]
fn sqlite_schema_contract_includes_events_table_and_indexes() {
    let ddl = create_schema_sql();

    assert!(ddl.contains("CREATE TABLE IF NOT EXISTS agentlog_events"));
    assert!(ddl.contains("idx_agentlog_events_run_sequence"));
    assert!(ddl.contains("idx_agentlog_events_timestamp"));
    assert!(ddl.contains("idx_agentlog_events_adapter_event"));
    assert!(ddl.contains("idx_agentlog_events_source"));
    assert!(ddl.contains("idx_agentlog_events_hashes"));
    assert!(ddl.contains("idx_agentlog_events_session_time"));
    assert!(ddl.contains("CREATE TABLE IF NOT EXISTS agentlog_schema_meta"));
}

#[test]
fn sqlite_schema_covers_canonical_agentlog_fields() {
    let ddl = create_schema_sql();
    for column in [
        "schema_version",
        "event_id",
        "run_id",
        "sequence_global",
        "source_kind",
        "source_path",
        "source_record_locator",
        "adapter_name",
        "record_format",
        "event_type",
        "role",
        "timestamp_utc",
        "timestamp_unix_ms",
        "timestamp_quality",
        "content_text",
        "tool_arguments_json",
        "tags_json",
        "warnings_json",
        "errors_json",
        "metadata_json",
        "raw_hash",
        "canonical_hash",
    ] {
        assert!(ddl.contains(column), "missing expected column: {column}");
    }
}

#[test]
fn insert_columns_are_stable_and_writer_ready() {
    assert_eq!(EVENT_INSERT_COLUMNS.len(), 44);
    assert_eq!(EVENT_INSERT_COLUMNS[0], "schema_version");
    assert_eq!(EVENT_INSERT_COLUMNS[1], "event_id");
    assert_eq!(EVENT_INSERT_COLUMNS[2], "run_id");
    assert_eq!(EVENT_INSERT_COLUMNS[43], "metadata_json");
    assert!(EVENT_INSERT_COLUMNS.contains(&"tags_json"));
    assert!(EVENT_INSERT_COLUMNS.contains(&"flags_json"));
    assert!(EVENT_INSERT_COLUMNS.contains(&"warnings_json"));
    assert!(EVENT_INSERT_COLUMNS.contains(&"errors_json"));
}

#[test]
fn schema_version_constant_matches_contract() {
    assert_eq!(SQLITE_SCHEMA_VERSION, "agentlog.v1.sqlite.v1");
    assert_eq!(SCHEMA_META_TABLE, "agentlog_schema_meta");
    assert_eq!(schema_statements().len(), 8);
}
