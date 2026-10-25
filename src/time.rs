use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub(crate) fn nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_now_seconds() {
        let ts = now_seconds();
        // Ensure the timestamp is reasonably recent (e.g., > 1.7 billion, which is roughly late 2023).
        assert!(ts > 1_700_000_000, "Timestamp should be recent, got: {}", ts);
    }

    #[test]
    fn test_nanos() {
        let ts = nanos();
        assert!(ts > 1_700_000_000_000_000_000, "Timestamp should be recent, got: {}", ts);
    }
}
