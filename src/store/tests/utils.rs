use crate::time::now_seconds;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn test_now_seconds() {
    let now = now_seconds();
    let sys_now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    assert!((sys_now - now).abs() <= 1, "now_seconds() should be close to SystemTime::now()");
    assert!(now > 1_700_000_000, "now_seconds() should return a valid recent timestamp");
}
