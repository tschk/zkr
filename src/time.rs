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
        let now = now_seconds();
        // 1_700_000_000 is roughly Nov 2023, ensuring we get a valid recent timestamp
        assert!(now > 1_700_000_000);

        // Ensure it doesn't return a timestamp in the far future
        let system_now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Allow a small buffer (e.g. 5 seconds) for execution time difference
        assert!(now <= system_now + 5);
    }
}
