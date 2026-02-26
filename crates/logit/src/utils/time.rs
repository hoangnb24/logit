use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Result, bail};
use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};

use crate::models::agentlog::TimestampQuality;

const EPOCH_SECONDS_CUTOFF: i128 = 100_000_000_000;
const EPOCH_MILLIS_CUTOFF: i128 = 100_000_000_000_000;
const EPOCH_MICROS_CUTOFF: i128 = 100_000_000_000_000_000;
const NANOS_PER_MILLI: i128 = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NormalizedTimestamp {
    pub timestamp_unix_ms: u64,
    pub timestamp_quality: TimestampQuality,
}

impl NormalizedTimestamp {
    #[must_use]
    pub fn timestamp_utc(self) -> String {
        format_unix_ms(self.timestamp_unix_ms)
    }
}

#[must_use]
pub fn unix_timestamp_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

pub fn normalize_timestamp_exact(raw: &str) -> Result<NormalizedTimestamp> {
    let normalized_ms = parse_timestamp_to_unix_ms(raw)?;
    Ok(NormalizedTimestamp {
        timestamp_unix_ms: normalized_ms,
        timestamp_quality: TimestampQuality::Exact,
    })
}

pub fn derive_timestamp(anchor_unix_ms: u64, offset_ms: u64) -> Result<NormalizedTimestamp> {
    let timestamp_unix_ms = anchor_unix_ms
        .checked_add(offset_ms)
        .ok_or_else(|| anyhow::anyhow!("derived timestamp overflow"))?;

    Ok(NormalizedTimestamp {
        timestamp_unix_ms,
        timestamp_quality: TimestampQuality::Derived,
    })
}

pub fn fallback_timestamp(
    run_started_at_utc: &str,
    sequence_offset_ms: u64,
) -> Result<NormalizedTimestamp> {
    let anchor = normalize_timestamp_exact(run_started_at_utc)?;
    let timestamp_unix_ms = anchor
        .timestamp_unix_ms
        .checked_add(sequence_offset_ms)
        .ok_or_else(|| anyhow::anyhow!("fallback timestamp overflow"))?;

    Ok(NormalizedTimestamp {
        timestamp_unix_ms,
        timestamp_quality: TimestampQuality::Fallback,
    })
}

pub fn parse_timestamp_to_unix_ms(raw: &str) -> Result<u64> {
    let candidate = raw.trim();
    if candidate.is_empty() {
        bail!("timestamp input is empty");
    }

    if let Ok(epoch_raw) = candidate.parse::<i128>() {
        return epoch_to_unix_ms(epoch_raw);
    }

    if let Ok(parsed) = OffsetDateTime::parse(candidate, &Rfc3339) {
        return to_unix_ms(parsed);
    }

    bail!("unsupported timestamp format: {candidate}");
}

pub fn format_unix_ms(timestamp_unix_ms: u64) -> String {
    let nanos = i128::from(timestamp_unix_ms)
        .checked_mul(NANOS_PER_MILLI)
        .unwrap_or(i128::MAX);
    let dt = OffsetDateTime::from_unix_timestamp_nanos(nanos)
        .expect("valid unix milliseconds must convert to datetime")
        .to_offset(UtcOffset::UTC);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        dt.year(),
        u8::from(dt.month()),
        dt.day(),
        dt.hour(),
        dt.minute(),
        dt.second(),
        dt.millisecond()
    )
}

fn epoch_to_unix_ms(epoch_raw: i128) -> Result<u64> {
    if epoch_raw < 0 {
        bail!("negative epoch values are not supported");
    }

    let epoch_ms = if epoch_raw < EPOCH_SECONDS_CUTOFF {
        epoch_raw.checked_mul(1_000)
    } else if epoch_raw < EPOCH_MILLIS_CUTOFF {
        Some(epoch_raw)
    } else if epoch_raw < EPOCH_MICROS_CUTOFF {
        Some(epoch_raw / 1_000)
    } else {
        Some(epoch_raw / 1_000_000)
    }
    .ok_or_else(|| anyhow::anyhow!("epoch conversion overflow"))?;

    u64::try_from(epoch_ms)
        .map_err(|_| anyhow::anyhow!("timestamp exceeds supported unix millisecond range"))
}

fn to_unix_ms(parsed: OffsetDateTime) -> Result<u64> {
    if parsed.unix_timestamp() < 0 {
        bail!("timestamps before 1970-01-01T00:00:00Z are not supported");
    }

    let unix_nanos = parsed.unix_timestamp_nanos();
    let unix_ms = unix_nanos / NANOS_PER_MILLI;
    u64::try_from(unix_ms)
        .map_err(|_| anyhow::anyhow!("timestamp exceeds supported unix millisecond range"))
}

#[cfg(test)]
mod tests {
    use super::{
        TimestampQuality, derive_timestamp, fallback_timestamp, format_unix_ms,
        normalize_timestamp_exact, parse_timestamp_to_unix_ms,
    };

    #[test]
    fn parses_rfc3339_utc() {
        let normalized =
            normalize_timestamp_exact("2026-02-05T07:00:03Z").expect("timestamp should parse");
        assert_eq!(normalized.timestamp_unix_ms, 1_770_274_803_000);
        assert_eq!(normalized.timestamp_quality, TimestampQuality::Exact);
        assert_eq!(normalized.timestamp_utc(), "2026-02-05T07:00:03.000Z");
    }

    #[test]
    fn parses_rfc3339_with_offset() {
        let as_utc = parse_timestamp_to_unix_ms("2026-02-05T09:00:03+02:00")
            .expect("timestamp should parse");
        assert_eq!(as_utc, 1_770_274_803_000);
    }

    #[test]
    fn infers_epoch_seconds() {
        let as_ms = parse_timestamp_to_unix_ms("1770274803").expect("seconds should parse");
        assert_eq!(as_ms, 1_770_274_803_000);
    }

    #[test]
    fn infers_epoch_millis() {
        let as_ms = parse_timestamp_to_unix_ms("1770274803000").expect("milliseconds should parse");
        assert_eq!(as_ms, 1_770_274_803_000);
    }

    #[test]
    fn infers_epoch_micros() {
        let as_ms =
            parse_timestamp_to_unix_ms("1770274803000000").expect("microseconds should parse");
        assert_eq!(as_ms, 1_770_274_803_000);
    }

    #[test]
    fn infers_epoch_nanos() {
        let as_ms =
            parse_timestamp_to_unix_ms("1770274803000000000").expect("nanoseconds should parse");
        assert_eq!(as_ms, 1_770_274_803_000);
    }

    #[test]
    fn derived_timestamp_adds_offset() {
        let derived =
            derive_timestamp(1_770_274_803_000, 25).expect("derived timestamp should compute");
        assert_eq!(derived.timestamp_unix_ms, 1_770_274_803_025);
        assert_eq!(derived.timestamp_quality, TimestampQuality::Derived);
    }

    #[test]
    fn fallback_timestamp_uses_run_anchor() {
        let fallback = fallback_timestamp("2026-02-05T07:00:03Z", 42)
            .expect("fallback timestamp should compute");
        assert_eq!(fallback.timestamp_unix_ms, 1_770_274_803_042);
        assert_eq!(fallback.timestamp_quality, TimestampQuality::Fallback);
        assert_eq!(
            format_unix_ms(fallback.timestamp_unix_ms),
            "2026-02-05T07:00:03.042Z"
        );
    }

    #[test]
    fn rejects_negative_epoch() {
        let err = parse_timestamp_to_unix_ms("-1").expect_err("negative epoch should fail");
        assert!(err.to_string().contains("negative epoch values"));
    }

    #[test]
    fn rejects_unsupported_string() {
        let err =
            parse_timestamp_to_unix_ms("next friday").expect_err("unsupported string should fail");
        assert!(err.to_string().contains("unsupported timestamp format"));
    }
}
