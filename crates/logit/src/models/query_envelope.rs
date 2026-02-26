use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::utils::time::{format_unix_ms, unix_timestamp_seconds};

pub const QUERY_ENVELOPE_SCHEMA_VERSION: &str = "logit.query-envelope.v1";

pub type QueryEnvelopeMeta = BTreeMap<String, Value>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryEnvelopeWarning {
    pub code: String,
    pub message: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryEnvelopeError {
    pub code: String,
    pub message: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryEnvelope {
    pub ok: bool,
    pub command: String,
    pub generated_at_utc: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,

    pub meta: QueryEnvelopeMeta,
    pub warnings: Vec<QueryEnvelopeWarning>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<QueryEnvelopeError>,
}

#[derive(Debug, Clone)]
pub struct QueryEnvelopeCommandFailure {
    envelope: QueryEnvelope,
}

impl QueryEnvelopeCommandFailure {
    #[must_use]
    pub fn new(envelope: QueryEnvelope) -> Self {
        Self { envelope }
    }

    #[must_use]
    pub fn envelope(&self) -> &QueryEnvelope {
        &self.envelope
    }
}

impl Display for QueryEnvelopeCommandFailure {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match serde_json::to_string(&self.envelope) {
            Ok(encoded) => f.write_str(&encoded),
            Err(_) => f.write_str("query envelope serialization failure"),
        }
    }
}

impl std::error::Error for QueryEnvelopeCommandFailure {}

impl QueryEnvelope {
    #[must_use]
    pub fn ok(command: impl Into<String>, data: Value) -> Self {
        Self::base(command, true).with_data(data)
    }

    #[must_use]
    pub fn error(
        command: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        let mut envelope = Self::base(command, false);
        envelope.error = Some(QueryEnvelopeError {
            code: code.into(),
            message: message.into(),
            details: None,
        });
        envelope
    }

    fn base(command: impl Into<String>, ok: bool) -> Self {
        let mut meta = QueryEnvelopeMeta::new();
        meta.insert(
            "schema_version".to_string(),
            json!(QUERY_ENVELOPE_SCHEMA_VERSION),
        );

        Self {
            ok,
            command: command.into(),
            generated_at_utc: generated_at_utc_now(),
            data: None,
            meta,
            warnings: Vec::new(),
            error: None,
        }
    }

    #[must_use]
    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }

    #[must_use]
    pub fn with_meta(mut self, key: impl Into<String>, value: Value) -> Self {
        self.meta.insert(key.into(), value);
        self
    }

    #[must_use]
    pub fn with_warning(mut self, code: impl Into<String>, message: impl Into<String>) -> Self {
        self.warnings.push(QueryEnvelopeWarning {
            code: code.into(),
            message: message.into(),
            details: None,
        });
        self
    }

    #[must_use]
    pub fn with_warning_details(mut self, details: Value) -> Self {
        if let Some(last_warning) = self.warnings.last_mut() {
            last_warning.details = Some(details);
        }
        self
    }

    #[must_use]
    pub fn with_error_details(mut self, details: Value) -> Self {
        if let Some(error) = self.error.as_mut() {
            error.details = Some(details);
        }
        self
    }
}

fn generated_at_utc_now() -> String {
    let now_ms = unix_timestamp_seconds().saturating_mul(1_000);
    format_unix_ms(now_ms)
}
