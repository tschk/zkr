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

#[allow(dead_code)]
pub(crate) fn parse_relative_time(s: &str) -> Option<i64> {
    if s.is_empty() {
        return None;
    }

    let s = s.trim();
    let mut num_end = 0;
    for (i, c) in s.char_indices() {
        if c.is_ascii_digit() || (i == 0 && (c == '-' || c == '+')) {
            num_end = i + c.len_utf8();
        } else {
            break;
        }
    }

    if num_end == 0 {
        return None;
    }

    let num_part = &s[..num_end];
    let unit_part = s[num_end..].trim();

    let value: i64 = num_part.parse().ok()?;

    let multiplier = match unit_part {
        "s" | "sec" | "secs" | "second" | "seconds" | "" => 1,
        "m" | "min" | "mins" | "minute" | "minutes" => 60,
        "h" | "hr" | "hrs" | "hour" | "hours" => 3600,
        "d" | "day" | "days" => 86400,
        "w" | "wk" | "wks" | "week" | "weeks" => 604800,
        "mo" | "mon" | "month" | "months" => 2592000,
        "y" | "yr" | "yrs" | "year" | "years" => 31536000,
        _ => return None,
    };

    value.checked_mul(multiplier)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_relative_time_seconds() {
        assert_eq!(parse_relative_time("5s"), Some(5));
        assert_eq!(parse_relative_time("10 sec"), Some(10));
        assert_eq!(parse_relative_time("-3 seconds"), Some(-3));
        assert_eq!(parse_relative_time("42"), Some(42));
    }

    #[test]
    fn test_parse_relative_time_minutes() {
        assert_eq!(parse_relative_time("5m"), Some(300));
        assert_eq!(parse_relative_time("2 min"), Some(120));
        assert_eq!(parse_relative_time("1 minute"), Some(60));
    }

    #[test]
    fn test_parse_relative_time_hours() {
        assert_eq!(parse_relative_time("2h"), Some(7200));
        assert_eq!(parse_relative_time("1 hour"), Some(3600));
    }

    #[test]
    fn test_parse_relative_time_days() {
        assert_eq!(parse_relative_time("1d"), Some(86400));
        assert_eq!(parse_relative_time("2 days"), Some(172800));
    }

    #[test]
    fn test_parse_relative_time_weeks() {
        assert_eq!(parse_relative_time("1w"), Some(604800));
        assert_eq!(parse_relative_time("3 weeks"), Some(1814400));
    }

    #[test]
    fn test_parse_relative_time_months_and_years() {
        assert_eq!(parse_relative_time("1mo"), Some(2592000));
        assert_eq!(parse_relative_time("1y"), Some(31536000));
    }

    #[test]
    fn test_parse_relative_time_invalid() {
        assert_eq!(parse_relative_time(""), None);
        assert_eq!(parse_relative_time("abc"), None);
        assert_eq!(parse_relative_time("10 invalid"), None);
        assert_eq!(parse_relative_time("- 10s"), None); // Space between minus and number
    }
}
