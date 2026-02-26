use std::path::Path;

use anyhow::{Context, Result};

pub use super::SourceClassification;
use super::{SourceFormatHint, classify_source};

const MAX_CLASSIFY_BYTES: usize = 256 * 1024;

#[must_use]
pub const fn classify_from_hint(hint: SourceFormatHint) -> Option<SourceClassification> {
    match hint {
        SourceFormatHint::Directory => None,
        SourceFormatHint::Json => Some(SourceClassification::Json),
        SourceFormatHint::Jsonl => Some(SourceClassification::Jsonl),
        SourceFormatHint::TextLog => Some(SourceClassification::TextLog),
    }
}

#[must_use]
pub fn classify_bytes(path: &Path, bytes: &[u8]) -> SourceClassification {
    classify_source(path, bytes)
}

pub fn classify_file(path: &Path) -> Result<SourceClassification> {
    let data = std::fs::read(path)
        .with_context(|| format!("failed to read source file for classification: {path:?}"))?;
    let sample_end = data.len().min(MAX_CLASSIFY_BYTES);
    Ok(classify_bytes(path, &data[..sample_end]))
}
