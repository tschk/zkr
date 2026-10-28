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
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let now = now_seconds();

        let after = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Ensure `now_seconds` returns a value between before and after
        assert!(now >= before);
        assert!(now <= after);

        // Ensure it's a reasonably recent timestamp (e.g., > Jan 1, 2024)
        assert!(now > 1_704_067_200);
    }

    #[test]
    fn test_nanos() {
        let ts = nanos();
        assert!(
            ts > 1_700_000_000_000_000_000,
            "Timestamp should be recent, got: {}",
            ts
        );
    }
}
