//! Shared reporting for Bybit's ADL (auto-deleveraging) rank.
//!
//! # Why this is not just a `log::warn!` at the call site
//!
//! ADL rank is a *state*, and it is re-reported on every position update. Logging it each time
//! produced ~2400 lines in six hours for two positions that never changed rank — the two of them
//! simply sat at the top of the ranking all day. Nothing was wrong with the logging rate; nothing was
//! looking at it.
//!
//! Two consequences shape this module:
//!
//! 1. **Emit on transition, not on observation.** A state re-stated hundreds of times an hour cannot
//!    drive an alert: firing on every line means paging continuously while the state persists, and
//!    suppressing repeats means building deduplication into the alert rule instead of into the
//!    signal. Entering, changing tier, and leaving are the events; the plateau between them is not.
//!
//! 2. **Carry the notional, not just the size.** Rank alone cannot say whether anything should be
//!    done. The rank is relative — the exchange orders positions by profit ratio × leverage, so the
//!    best-performing position is always near the top, and for a small position the only sensible
//!    response is to do nothing. `size` cannot fill that gap either: it counts coins, and 2.16 of one
//!    instrument versus 392.5 of another are not comparable and cannot share a threshold. Notional is
//!    what an alert can actually threshold on.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Bybit ranks open positions 1–5 by ADL priority (5 = next to be deleveraged); 0 means flat.
/// 4 is the point at which the venue is close enough to force-closing that a human might act.
pub const ELEVATED_RANK: i32 = 4;

/// Quote-currency notional below which an elevated rank is reported at INFO instead of WARN.
///
/// Deferring this judgement entirely to the alert layer (as the module doc above originally
/// proposed) does not work: dedicated rules can threshold on `position_notional`, but a CATCH-ALL
/// "any WARN" rule cannot, and that is the one that actually pages. Observed live: STORJUSDT at
/// $16.22 notional oscillating between rank 4 and 5, one WARN per flip, none of them actionable —
/// the worst case is losing a fraction of $16.
///
/// The value matches the `adl-critical` / `adl-notice` split (`position_notional > 500`) so the log
/// level and the alert rules cannot drift apart. The record is still written either way; only the
/// claim on an operator's attention changes.
pub const ACTIONABLE_NOTIONAL: f64 = 500.0;

/// Last rank reported per instrument, so only transitions are logged.
static LAST_RANK: OnceLock<Mutex<HashMap<String, i32>>> = OnceLock::new();

fn last_rank() -> &'static Mutex<HashMap<String, i32>> {
    LAST_RANK.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Whether an elevated rank on this position deserves an operator's attention.
///
/// Actionable **unless we can prove the position is small**: an unparseable, empty or absent
/// notional reports as actionable, because the failure that costs money is staying quiet about a
/// big position, not being noisy about an unreadable one.
fn is_actionable(position_value: &str) -> bool {
    !position_value
        .trim()
        .parse::<f64>()
        .is_ok_and(|v| v < ACTIONABLE_NOTIONAL)
}

/// Reports an observed ADL rank, logging only when it changes.
///
/// `position_value` is Bybit's `positionValue` (quote-currency notional) passed through verbatim;
/// parsing it here would mean inventing a policy for the empty and `"0"` cases that the caller
/// already handles for every other field.
pub fn report(instrument_id: &str, rank: i32, size: &str, position_value: &str) {
    // Ignore poisoning: a panic elsewhere must not silently switch this reporting off.
    let mut map = last_rank().lock().unwrap_or_else(|e| e.into_inner());
    let previous = map.get(instrument_id).copied().unwrap_or(0);
    if previous == rank {
        return;
    }
    map.insert(instrument_id.to_string(), rank);

    if rank >= ELEVATED_RANK {
        let actionable = is_actionable(position_value);
        // Structured fields, not an interpolated sentence: an alert has to compare the rank and the
        // notional numerically, and values baked into message text can only be string-matched.
        // Same message and fields at both levels — the alert rules key off the fields, and only the
        // level decides whether this interrupts anybody.
        if actionable {
            log::warn!(
                adl_rank = rank,
                adl_rank_previous = previous,
                symbol = instrument_id,
                position_size = size,
                position_notional = position_value;
                "Elevated ADL risk"
            );
        } else {
            log::info!(
                adl_rank = rank,
                adl_rank_previous = previous,
                symbol = instrument_id,
                position_size = size,
                position_notional = position_value;
                "Elevated ADL risk"
            );
        }
    } else if previous >= ELEVATED_RANK {
        // The recovery edge matters as much as the onset: without it an alert can only ever fire, and
        // whoever is watching has no way to learn the situation resolved itself.
        log::info!(
            adl_rank = rank,
            adl_rank_previous = previous,
            symbol = instrument_id,
            position_size = size,
            position_notional = position_value;
            "ADL risk cleared"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// State is process-global here; these cases must not interleave.
    static SERIAL: Mutex<()> = Mutex::new(());

    fn reset() {
        last_rank().lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    fn seen(instrument: &str) -> Option<i32> {
        last_rank()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(instrument)
            .copied()
    }

    /// The whole point: a rank that stays put must be recorded once, not on every position update.
    #[test]
    fn repeated_identical_rank_is_recorded_once() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        report("STORJUSDT-LINEAR.BYBIT", 5, "392.5", "110.0");
        assert_eq!(seen("STORJUSDT-LINEAR.BYBIT"), Some(5));

        // Same rank again — state unchanged, so nothing new to say.
        report("STORJUSDT-LINEAR.BYBIT", 5, "392.5", "111.0");
        assert_eq!(seen("STORJUSDT-LINEAR.BYBIT"), Some(5));
    }

    /// Moving between elevated tiers is a real change and must not be swallowed by "already high".
    #[test]
    fn tier_change_within_the_elevated_band_is_a_transition() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        report("TRBUSDT-LINEAR.BYBIT", 4, "2.16", "80.0");
        report("TRBUSDT-LINEAR.BYBIT", 5, "2.16", "80.0");
        assert_eq!(seen("TRBUSDT-LINEAR.BYBIT"), Some(5));
    }

    /// Dropping out of the elevated band is tracked too, so recovery is observable.
    #[test]
    fn dropping_below_the_threshold_is_tracked() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        report("APEUSDT-LINEAR.BYBIT", 5, "10", "50.0");
        report("APEUSDT-LINEAR.BYBIT", 2, "10", "50.0");
        assert_eq!(seen("APEUSDT-LINEAR.BYBIT"), Some(2));
    }

    /// The live case that motivated the split: STORJUSDT at $16.22 flipping between rank 4 and 5.
    /// Each flip is a genuine transition and must still be recorded — just not at WARN, where the
    /// catch-all "any WARN" alert picks it up and pages about a position worth $16.
    #[test]
    fn a_small_position_is_not_actionable() {
        assert!(!is_actionable("16.2181"));
        assert!(!is_actionable("499.99"));
        assert!(is_actionable("500.0"));
        assert!(is_actionable("12345.6"));
    }

    /// Actionable unless proven small — an unreadable notional must not buy silence.
    #[test]
    fn an_unreadable_notional_is_treated_as_actionable() {
        assert!(is_actionable(""));
        assert!(is_actionable("   "));
        assert!(is_actionable("n/a"));
        // ...but a well-formed small value with padding still reads as small.
        assert!(!is_actionable("  16.22  "));
    }

    /// The threshold must equal the one the alert rules split on, or the log level and the alerting
    /// drift apart and each looks correct on its own.
    #[test]
    fn threshold_matches_the_alert_rules() {
        assert_eq!(ACTIONABLE_NOTIONAL, 500.0);
    }

    /// Instruments must not share state — one going quiet cannot mask another's transition.
    #[test]
    fn instruments_are_tracked_independently() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        report("A-LINEAR.BYBIT", 5, "1", "10.0");
        report("B-LINEAR.BYBIT", 4, "1", "10.0");
        assert_eq!(seen("A-LINEAR.BYBIT"), Some(5));
        assert_eq!(seen("B-LINEAR.BYBIT"), Some(4));
    }
}
