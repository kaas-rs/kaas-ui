//! Numbers a person reads, and prose that agrees with them.
//!
//! Integer arithmetic throughout for the durations and sizes:
//! `as_conversions` is denied at the workspace root and a rounded string is
//! not worth an exception. The two float helpers at the bottom carry the one
//! `allow` between them, which is the shape `analysis.rs` already established
//! — confine the cast so the lint keeps guarding everywhere it matters.

const SECOND_MS: i64 = 1_000;
const MINUTE_MS: i64 = 60 * SECOND_MS;
const HOUR_MS: i64 = 60 * MINUTE_MS;
const DAY_MS: i64 = 24 * HOUR_MS;

/// A duration in the largest unit that leaves it above one.
///
/// Integer arithmetic throughout — `as_conversions` is denied at the
/// workspace root and a rounded string is not worth an exception.
pub fn human_ms(ms: i64) -> String {
    if ms < 0 {
        return "unlimited".to_owned();
    }
    for (unit, name) in [
        (DAY_MS, "d"),
        (HOUR_MS, "h"),
        (MINUTE_MS, "min"),
        (SECOND_MS, "s"),
    ] {
        if ms >= unit {
            return scaled(ms, unit, name);
        }
    }
    format!("{ms} ms")
}

const KIB: i64 = 1024;

/// Bytes, in binary units.
pub fn human_bytes(bytes: i64) -> String {
    if bytes < 0 {
        return "unlimited".to_owned();
    }
    let mut unit = KIB * KIB * KIB * KIB;
    for name in ["TiB", "GiB", "MiB", "KiB"] {
        if bytes >= unit {
            return scaled(bytes, unit, name);
        }
        unit /= KIB;
    }
    format!("{bytes} B")
}

/// `value / unit` to one decimal place, without touching a float.
///
/// The tenth is dropped when it is zero, so a round number renders round.
fn scaled(value: i64, unit: i64, name: &str) -> String {
    let whole = value / unit;
    let tenths = value.saturating_mul(10) / unit - whole.saturating_mul(10);
    if tenths == 0 {
        format!("{whole} {name}")
    } else {
        format!("{whole}.{tenths} {name}")
    }
}

/// "the one measured partition holds" / "2 of 5 measured partitions hold",
/// with the segment clause and the pronoun that agree with it.
///
/// Prose assembled from numbers reads as machine output the moment it says
/// "1 partitions", and a reader who notices that stops trusting the number in
/// front of it.
pub fn holds(stuck: usize, measured: usize) -> (String, &'static str, &'static str) {
    let (segments, pronoun) = if stuck == 1 {
        ("that segment stays open", "it")
    } else {
        ("those segments stay open", "them")
    };
    let phrase = if measured == 1 {
        "the one measured partition holds".to_owned()
    } else if stuck == measured {
        format!("all {measured} measured partitions hold")
    } else {
        format!("{stuck} of {measured} measured partitions hold")
    };
    (phrase, segments, pronoun)
}

/// `u64` into the float arithmetic the rate derivations need, in one place.
///
/// Lossy above 2^53, and every number that passes through here is a rate or a
/// ratio whose own uncertainty dwarfs that.
#[allow(clippy::as_conversions, clippy::cast_precision_loss)]
pub fn to_f64(value: u64) -> f64 {
    value as f64
}

/// `i64` the same way.
#[allow(clippy::as_conversions, clippy::cast_precision_loss)]
pub fn i64_to_f64(value: i64) -> f64 {
    value as f64
}

/// Back to a whole number of bytes, or `None` when the float is not one.
///
/// Every recommendation that ends in a byte count comes through here, so a
/// rate that overflowed or went negative becomes "not enough data" rather
/// than a number nobody can explain.
#[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
pub fn to_i64(value: f64) -> Option<i64> {
    if !value.is_finite() || value < 0.0 || value > i64_to_f64(i64::MAX) {
        return None;
    }
    Some(value as i64)
}

/// The largest power of two at or below `value`, and never below `floor`.
///
/// Segment sizes are read by people and typed into configuration files;
/// 73400320 is a number nobody chooses on purpose.
#[must_use]
pub fn round_to_power_of_two(value: i64, floor: i64) -> i64 {
    if value <= floor {
        return floor;
    }
    let bits = 63 - value.leading_zeros();
    1i64.checked_shl(bits).unwrap_or(floor).max(floor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_and_sizes_render_in_the_unit_a_person_reads() {
        assert_eq!(human_ms(86_400_000), "1 d");
        assert_eq!(human_ms(604_800_000), "7 d");
        assert_eq!(human_ms(5_400_000), "1.5 h");
        assert_eq!(human_ms(500), "500 ms");
        assert_eq!(human_ms(-1), "unlimited");
        assert_eq!(human_bytes(1_073_741_824), "1 GiB");
        assert_eq!(human_bytes(1_610_612_736), "1.5 GiB");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(-1), "unlimited");
    }

    #[test]
    fn prose_agrees_with_the_number_in_front_of_it() {
        assert_eq!(holds(1, 1).0, "the one measured partition holds");
        assert_eq!(holds(1, 1).2, "it");
        assert_eq!(holds(1, 1).1, "that segment stays open");
        assert_eq!(holds(3, 3).0, "all 3 measured partitions hold");
        assert_eq!(holds(3, 3).2, "them");
        assert_eq!(holds(1, 4).0, "1 of 4 measured partitions hold");
    }

    #[test]
    fn a_segment_size_rounds_to_something_a_person_would_type() {
        assert_eq!(round_to_power_of_two(100_000_000, 1024), 67_108_864);
        assert_eq!(round_to_power_of_two(67_108_864, 1024), 67_108_864);
        assert_eq!(round_to_power_of_two(5, 1024), 1024, "the floor wins");
    }

    #[test]
    fn a_rate_that_is_not_a_number_is_not_a_recommendation() {
        assert_eq!(to_i64(f64::NAN), None);
        assert_eq!(to_i64(f64::INFINITY), None);
        assert_eq!(to_i64(-1.0), None);
        assert_eq!(to_i64(4096.9), Some(4096));
    }
}
