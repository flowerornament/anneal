use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn current_days_since_epoch() -> Option<i64> {
    let duration = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(duration.as_secs() / 86_400).ok()
}

pub(crate) fn snapshot_days_since_epoch(value: &str) -> Option<i64> {
    let date = value.get(0.."YYYY-MM-DD".len())?;
    iso_days_since_epoch(date)
}

pub(crate) fn relative_days_reference(reference: &str) -> Option<i64> {
    let days = reference
        .strip_prefix("--")?
        .strip_suffix("days")?
        .parse::<i64>()
        .ok()?;
    if days < 0 {
        return None;
    }
    current_days_since_epoch().map(|today| today.saturating_sub(days))
}

pub(crate) fn iso_days_since_epoch(value: &str) -> Option<i64> {
    if value.len() != "YYYY-MM-DD".len() {
        return None;
    }
    let year = value.get(0..4)?.parse::<i32>().ok()?;
    let month = value.get(5..7)?.parse::<u32>().ok()?;
    let day = value.get(8..10)?.parse::<u32>().ok()?;
    if value.as_bytes().get(4) != Some(&b'-') || value.as_bytes().get(7) != Some(&b'-') {
        return None;
    }
    days_from_civil(year, month, day)
}

fn days_from_civil(year: i32, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
        return None;
    }
    let month = i64::from(month);
    let day = i64::from(day);
    let year = i64::from(year) - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_prime = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(era * 146_097 + day_of_era - 719_468)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}
