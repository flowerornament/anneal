//! Time parsing and snapshot reference helpers.

use std::fmt;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::de;
use serde::{Deserialize, Serialize};

const ISO_DATE_LEN: usize = "YYYY-MM-DD".len();
const RFC3339_TIME_START: usize = "YYYY-MM-DDT".len();
const RFC3339_TIME_END: usize = "YYYY-MM-DDTHH:MM:SS".len();

/// Validated wire-preserving timestamp for runtime snapshots.
///
/// Ordering is deliberately absent: existing selection compares raw wire text
/// within day and snapshot ties, while a semantic total order would silently
/// change that behavior.
///
/// ```compile_fail
/// use anneal_core::runtime::SnapshotTime;
///
/// fn require_order<T: Ord>() {}
/// require_order::<SnapshotTime>();
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotTime(Arc<SnapshotTimeInner>);

#[derive(Debug, PartialEq, Eq)]
struct SnapshotTimeInner {
    wire: Arc<str>,
    day_since_epoch: i64,
}

impl SnapshotTime {
    /// Parse an exact ISO date or the runtime's accepted RFC3339 subset.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotTimeError`] when the complete input does not match
    /// either accepted wire form.
    pub fn parse(value: impl Into<String>) -> Result<Self, SnapshotTimeError> {
        let value = value.into();
        let Some(day_since_epoch) = snapshot_days_since_epoch(&value) else {
            return Err(SnapshotTimeError { value });
        };
        Ok(Self(Arc::new(SnapshotTimeInner {
            wire: Arc::from(value),
            day_since_epoch,
        })))
    }

    /// Return the original, unnormalized wire text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0.wire
    }

    /// Return the calendar day encoded by the wire text.
    ///
    /// This cache has no independent authority: parsing derives it from
    /// `as_str`, and no constructor accepts the two representations separately.
    #[must_use]
    pub fn day_since_epoch(&self) -> i64 {
        self.0.day_since_epoch
    }
}

impl TryFrom<String> for SnapshotTime {
    type Error = SnapshotTimeError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl TryFrom<&str> for SnapshotTime {
    type Error = SnapshotTimeError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl AsRef<str> for SnapshotTime {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for SnapshotTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for SnapshotTime {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for SnapshotTime {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(de::Error::custom)
    }
}

/// Invalid snapshot timestamp wire text.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("snapshot timestamp {value:?} is not parseable")]
pub struct SnapshotTimeError {
    value: String,
}

pub(crate) fn current_days_since_epoch() -> Option<i64> {
    let duration = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(duration.as_secs() / 86_400).ok()
}

pub(crate) fn snapshot_days_since_epoch(value: &str) -> Option<i64> {
    if value.len() == ISO_DATE_LEN {
        return iso_days_since_epoch(value);
    }
    rfc3339_days_since_epoch(value)
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
    if value.len() != ISO_DATE_LEN {
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

fn rfc3339_days_since_epoch(value: &str) -> Option<i64> {
    if value.len() < RFC3339_TIME_END + 1 {
        return None;
    }
    if value.as_bytes().get(ISO_DATE_LEN) != Some(&b'T') {
        return None;
    }
    let day = iso_days_since_epoch(value.get(0..ISO_DATE_LEN)?)?;
    validate_rfc3339_time(value)?;
    Some(day)
}

fn validate_rfc3339_time(value: &str) -> Option<()> {
    if value.as_bytes().get(13) != Some(&b':') || value.as_bytes().get(16) != Some(&b':') {
        return None;
    }
    let hour = value.get(RFC3339_TIME_START..13)?.parse::<u32>().ok()?;
    let minute = value.get(14..16)?.parse::<u32>().ok()?;
    let second = value.get(17..RFC3339_TIME_END)?.parse::<u32>().ok()?;
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }

    let bytes = value.as_bytes();
    let mut zone_index = RFC3339_TIME_END;
    if bytes.get(zone_index) == Some(&b'.') {
        zone_index += 1;
        let fraction_start = zone_index;
        while bytes.get(zone_index).is_some_and(u8::is_ascii_digit) {
            zone_index += 1;
        }
        if zone_index == fraction_start {
            return None;
        }
    }
    validate_rfc3339_zone(value.get(zone_index..)?)
}

fn validate_rfc3339_zone(zone: &str) -> Option<()> {
    if zone == "Z" {
        return Some(());
    }
    if zone.len() != "+HH:MM".len() {
        return None;
    }
    let bytes = zone.as_bytes();
    if !matches!(bytes.first(), Some(b'+' | b'-')) || bytes.get(3) != Some(&b':') {
        return None;
    }
    let hour = zone.get(1..3)?.parse::<u32>().ok()?;
    let minute = zone.get(4..6)?.parse::<u32>().ok()?;
    if hour > 23 || minute > 59 {
        return None;
    }
    Some(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_days_accepts_exact_date_or_rfc3339_timestamp() {
        let day = snapshot_days_since_epoch("2026-05-13").expect("date parses");

        assert_eq!(snapshot_days_since_epoch("2026-05-13T10:30:45Z"), Some(day));
        assert_eq!(
            snapshot_days_since_epoch("2026-05-13T10:30:45.123+02:30"),
            Some(day)
        );
    }

    #[test]
    fn snapshot_days_rejects_valid_date_prefix_with_suffix() {
        assert_eq!(snapshot_days_since_epoch("2026-05-13junk"), None);
        assert_eq!(snapshot_days_since_epoch("2026-05-13T10:30:45Zjunk"), None);
        assert_eq!(snapshot_days_since_epoch("2026-05-13T24:00:00Z"), None);
        assert_eq!(snapshot_days_since_epoch("2026-05-13 10:30:45Z"), None);
    }

    #[test]
    fn snapshot_time_preserves_wire_bytes_and_derived_day() {
        let value = "2026-05-13T10:30:45.123+02:30";
        let time = SnapshotTime::parse(value).expect("timestamp parses");
        let clone = time.clone();

        assert_eq!(time.as_str(), value);
        assert_eq!(
            time.day_since_epoch(),
            snapshot_days_since_epoch(value).expect("day parses")
        );
        assert!(Arc::ptr_eq(&time.0, &clone.0));
        assert_eq!(
            serde_json::to_string(&time).expect("timestamp serializes"),
            format!("{value:?}")
        );
        assert_eq!(
            serde_json::from_str::<SnapshotTime>(&format!("{value:?}"))
                .expect("timestamp deserializes"),
            time
        );
    }

    #[test]
    fn snapshot_time_is_one_shared_pointer() {
        assert_eq!(
            std::mem::size_of::<SnapshotTime>(),
            std::mem::size_of::<usize>()
        );
    }
}
