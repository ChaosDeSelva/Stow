//! Ids and timestamps. Ids are ULIDs; timestamps are canonical RFC 3339 UTC strings with
//! microseconds and a `Z` suffix, so lexical order equals chronological order.

use chrono::{DateTime, SecondsFormat, Utc};

use crate::{Result, StowError};

/// Crockford base32 alphabet used by ULIDs (no I, L, O, U).
const CROCKFORD: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// ULID string: 26 chars, lexically sortable by creation time.
pub fn new_id() -> String {
    ulid::Ulid::generate().to_string()
}

/// Current time in canonical form, e.g. `2026-11-08T18:02:11.123456Z`.
pub fn now() -> String {
    canonical(Utc::now())
}

fn canonical(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Micros, true)
}

/// Validate any RFC 3339 timestamp and return it in canonical UTC form
/// (`YYYY-MM-DDTHH:MM:SS.ffffffZ`). Invalid input → [`StowError::InvalidInput`].
pub fn parse_rfc3339(s: &str) -> Result<String> {
    DateTime::parse_from_rfc3339(s.trim())
        .map(|t| canonical(t.with_timezone(&Utc)))
        .map_err(|_| StowError::invalid("timestamp", s))
}

/// 26 Crockford base32 characters (either case) whose first digit fits 128 bits.
pub fn is_ulid(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 26
        && b[0].to_ascii_uppercase() <= b'7'
        && b.iter()
            .all(|c| CROCKFORD.contains(&c.to_ascii_uppercase()))
}

/// [`is_ulid`] or [`StowError::InvalidInput`] naming `field`.
pub fn require_ulid(field: &'static str, s: &str) -> Result<()> {
    if is_ulid(s) {
        Ok(())
    } else {
        Err(StowError::invalid(field, s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_id_is_26_chars_and_sorts() {
        let a = new_id();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b = new_id();
        assert_eq!(a.len(), 26);
        assert!(a < b, "{a} should sort before {b}");
        assert!(is_ulid(&a));
    }

    #[test]
    fn now_parses_as_rfc3339() {
        let s = now();
        let parsed = chrono::DateTime::parse_from_rfc3339(&s).expect("rfc3339");
        assert_eq!(parsed.offset().local_minus_utc(), 0);
        assert_eq!(parse_rfc3339(&s).unwrap(), s, "now() is already canonical");
    }

    #[test]
    fn parse_rfc3339_normalizes_to_utc_micros() {
        assert_eq!(
            parse_rfc3339("2026-01-01T00:00:00Z").unwrap(),
            "2026-01-01T00:00:00.000000Z"
        );
        assert_eq!(
            parse_rfc3339("2026-01-01T07:30:00.5-05:00").unwrap(),
            "2026-01-01T12:30:00.500000Z"
        );
        assert_eq!(
            parse_rfc3339("2026-01-01T00:00:00.123+00:00").unwrap(),
            "2026-01-01T00:00:00.123000Z"
        );
        for bad in ["", "tomorrow", "2026-01-01", "2026-13-01T00:00:00Z"] {
            let err = parse_rfc3339(bad).unwrap_err();
            assert_eq!(err.code(), "invalid_input", "{bad}");
        }
    }

    #[test]
    fn ulid_validation() {
        assert!(is_ulid("01HZY3V8Q6K9J2M4N5P7R8S9TA"));
        assert!(is_ulid("01hzy3v8q6k9j2m4n5p7r8s9ta"), "lowercase accepted");
        assert!(!is_ulid("01HZY3V8Q6K9J2M4N5P7R8S9T"), "25 chars");
        assert!(!is_ulid("01HZY3V8Q6K9J2M4N5P7R8S9TAA"), "27 chars");
        assert!(!is_ulid("01HZY3V8Q6K9J2M4N5P7R8S9TI"), "I is not Crockford");
        assert!(!is_ulid("81HZY3V8Q6K9J2M4N5P7R8S9TA"), "overflows 128 bits");
        assert!(!is_ulid("../../etc/passwd"));
        assert!(require_ulid("id", &new_id()).is_ok());
        let err = require_ulid("bucket_id", "nope").unwrap_err();
        assert!(
            matches!(err, StowError::InvalidInput { field: "bucket_id", ref value } if value == "nope")
        );
    }
}
