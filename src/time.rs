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
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let mut chars = s.chars().peekable();

    let sign: i64 = match chars.peek() {
        Some(&'-') => {
            chars.next();
            -1
        }
        Some(&'+') => {
            chars.next();
            1
        }
        _ => 1,
    };

    let mut num_str = String::new();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            num_str.push(c);
            chars.next();
        } else {
            break;
        }
    }

    if num_str.is_empty() {
        return None;
    }

    let num: i64 = num_str.parse().ok()?;

    let unit_str = chars.collect::<String>();
    let unit = unit_str.trim();

    let multiplier: i64 = match unit {
        "s" | "sec" | "secs" | "second" | "seconds" | "" => 1,
        "m" | "min" | "mins" | "minute" | "minutes" => 60,
        "h" | "hr" | "hrs" | "hour" | "hours" => 3600,
        "d" | "day" | "days" => 86400,
        "w" | "wk" | "wks" | "week" | "weeks" => 604800,
        "M" | "mo" | "mos" | "month" | "months" => 2592000,
        "y" | "yr" | "yrs" | "year" | "years" => 31536000,
        _ => return None,
    };

    sign.checked_mul(num)?.checked_mul(multiplier)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_relative_time_happy_paths() {
        assert_eq!(parse_relative_time("10"), Some(10));
        assert_eq!(parse_relative_time("10s"), Some(10));
        assert_eq!(parse_relative_time("10sec"), Some(10));
        assert_eq!(parse_relative_time("10secs"), Some(10));
        assert_eq!(parse_relative_time("10second"), Some(10));
        assert_eq!(parse_relative_time("10seconds"), Some(10));

        assert_eq!(parse_relative_time("5m"), Some(300));
        assert_eq!(parse_relative_time("5min"), Some(300));
        assert_eq!(parse_relative_time("5mins"), Some(300));
        assert_eq!(parse_relative_time("5minute"), Some(300));
        assert_eq!(parse_relative_time("5minutes"), Some(300));

        assert_eq!(parse_relative_time("2h"), Some(7200));
        assert_eq!(parse_relative_time("2hr"), Some(7200));
        assert_eq!(parse_relative_time("2hrs"), Some(7200));
        assert_eq!(parse_relative_time("2hour"), Some(7200));
        assert_eq!(parse_relative_time("2hours"), Some(7200));

        assert_eq!(parse_relative_time("1d"), Some(86400));
        assert_eq!(parse_relative_time("1day"), Some(86400));
        assert_eq!(parse_relative_time("1days"), Some(86400));

        assert_eq!(parse_relative_time("2w"), Some(1209600));
        assert_eq!(parse_relative_time("2wk"), Some(1209600));
        assert_eq!(parse_relative_time("2wks"), Some(1209600));
        assert_eq!(parse_relative_time("2week"), Some(1209600));
        assert_eq!(parse_relative_time("2weeks"), Some(1209600));

        assert_eq!(parse_relative_time("1M"), Some(2592000));
        assert_eq!(parse_relative_time("1mo"), Some(2592000));
        assert_eq!(parse_relative_time("1mos"), Some(2592000));
        assert_eq!(parse_relative_time("1month"), Some(2592000));
        assert_eq!(parse_relative_time("1months"), Some(2592000));

        assert_eq!(parse_relative_time("1y"), Some(31536000));
        assert_eq!(parse_relative_time("1yr"), Some(31536000));
        assert_eq!(parse_relative_time("1yrs"), Some(31536000));
        assert_eq!(parse_relative_time("1year"), Some(31536000));
        assert_eq!(parse_relative_time("1years"), Some(31536000));

        // Signs
        assert_eq!(parse_relative_time("-5m"), Some(-300));
        assert_eq!(parse_relative_time("+5m"), Some(300));

        // Whitespace
        assert_eq!(parse_relative_time("  10  "), Some(10));
        assert_eq!(parse_relative_time("\t10s\n"), Some(10));
    }

    #[test]
    fn test_parse_relative_time_edge_cases_and_errors() {
        assert_eq!(parse_relative_time(""), None);
        assert_eq!(parse_relative_time("   "), None);
        assert_eq!(parse_relative_time("abc"), None);
        assert_eq!(parse_relative_time("10abc"), None);
        assert_eq!(parse_relative_time("-"), None);
        assert_eq!(parse_relative_time("+"), None);

        // Overflow edge cases (simulated with very large numbers, parse will return None)
        assert_eq!(parse_relative_time("99999999999999999999999999s"), None);

        // Invalid units
        assert_eq!(parse_relative_time("10x"), None);

        assert_eq!(parse_relative_time("-10  s"), Some(-10));
    }
}
