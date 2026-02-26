use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SCHEMA_VERSION: &str = "agentlog.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum SchemaVersion {
    #[serde(rename = "agentlog.v1")]
    #[schemars(rename = "agentlog.v1")]
    AgentLogV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentSource {
    Codex,
    Claude,
    Gemini,
    Amp,
    OpenCode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecordFormat {
    Message,
    ToolCall,
    ToolResult,
    System,
    Diagnostic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    Prompt,
    Response,
    SystemNotice,
    ToolInvocation,
    ToolOutput,
    StatusUpdate,
    Error,
    Metric,
    ArtifactReference,
    DebugLog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActorRole {
    User,
    Assistant,
    System,
    Tool,
    Runtime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TimestampQuality {
    Exact,
    Derived,
    Fallback,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentLogEvent {
    pub schema_version: SchemaVersion,
    pub event_id: String,
    pub run_id: String,
    pub sequence_global: u64,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence_source: Option<u64>,

    pub source_kind: AgentSource,
    pub source_path: String,
    pub source_record_locator: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_record_hash: Option<String>,

    pub adapter_name: AgentSource,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub adapter_version: Option<String>,

    pub record_format: RecordFormat,
    pub event_type: EventType,
    pub role: ActorRole,
    pub timestamp_utc: String,
    pub timestamp_unix_ms: u64,
    pub timestamp_quality: TimestampQuality,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_event_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor_name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_text: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_excerpt: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_mime: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_arguments_json: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_result_text: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub pii_redacted: Option<bool>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,

    pub raw_hash: String,
    pub canonical_hash: String,

    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, Value>,
}

#[must_use]
pub fn json_schema() -> Value {
    let schema = schemars::schema_for!(AgentLogEvent);
    match serde_json::to_value(schema) {
        Ok(value) => value,
        Err(error) => {
            panic!("failed to serialize generated agentlog schema: {error}");
        }
    }
}
