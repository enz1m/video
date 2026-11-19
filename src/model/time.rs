//! Times and rates as the player shows them.

use std::time::Duration;

pub(crate) const RATES: [f64; 8] = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0];

// Floor seconds like media controls; duration-based hour width prevents position jitter.
pub(crate) fn clock_ascii(at: Duration, long: bool) -> String {
    let total = at.as_secs();
    let (hours, minutes, seconds) = (total / 3600, total / 60 % 60, total % 60);
    if long || hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

pub(crate) fn clock(at: Duration, long: bool) -> String {
    clock_ascii(at, long)
        .split(':')
        .map(crate::l10n::decimal)
        .collect::<Vec<_>>()
        .join(":")
}

pub(crate) fn rate_label(rate: f64) -> String {
    let text = format!("{rate:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    format!("{}×", crate::l10n::decimal(text))
}

pub(crate) fn step_rate(rate: f64, faster: bool) -> Option<f64> {
    if faster {
        RATES.iter().copied().find(|offered| *offered > rate + 1e-6)
    } else {
        RATES
            .iter()
            .rev()
            .copied()
            .find(|offered| *offered < rate - 1e-6)
    }
}

pub(crate) fn offset(position: Duration, delta_ms: i64, duration: Option<Duration>) -> Duration {
    let moved = if delta_ms < 0 {
        position.saturating_sub(Duration::from_millis(delta_ms.unsigned_abs()))
    } else {
        position.saturating_add(Duration::from_millis(delta_ms.unsigned_abs()))
    };
    duration.map_or(moved, |duration| moved.min(duration))
}
