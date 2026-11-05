//! Small shared helpers.

pub fn fmt_bytes(b: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{b} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

/// Convert a CIM datetime (`yyyymmddHHMMSS.mmmmmm+UUU`) to RFC 3339 UTC.
pub fn cim_datetime(s: &str) -> Option<String> {
    if s.len() < 14 {
        return None;
    }
    let naive = chrono::NaiveDateTime::parse_from_str(&s[..14], "%Y%m%d%H%M%S").ok()?;
    // Offset in minutes after the sign at position 21.
    let offset_min: i64 = s
        .get(21..)
        .and_then(|o| o.trim().parse::<i64>().ok())
        .unwrap_or(0);
    let utc = naive - chrono::Duration::minutes(offset_min);
    Some(utc.and_utc().to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

/// Clamp a string to `max` chars on a char boundary.
pub fn clamp(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes() {
        assert_eq!(fmt_bytes(512), "512 B");
        assert_eq!(fmt_bytes(1536), "1.5 KB");
        assert_eq!(fmt_bytes(16 * 1024 * 1024 * 1024), "16.0 GB");
    }

    #[test]
    fn cim() {
        assert_eq!(cim_datetime("20261105101500.000000+060").as_deref(), Some("2026-11-05T09:15:00Z"));
        assert_eq!(cim_datetime("20261105101500.000000+000").as_deref(), Some("2026-11-05T10:15:00Z"));
        assert_eq!(cim_datetime("bad"), None);
    }
}
