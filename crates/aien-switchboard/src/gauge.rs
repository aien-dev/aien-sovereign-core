//! Usage gauges: how much room an account has, as far as we know.

use crate::account::AccountId;
use std::collections::BTreeMap;

/// One usage window, for example a five-hour or seven-day window.
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    pub name: String,
    /// 0.0 to 100.0.
    pub used_percent: f64,
    /// Unix seconds when the window refills, when known.
    pub resets_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Gauge {
    pub windows: Vec<Window>,
    /// The account is treated as full until this Unix second.
    pub cooled_until: Option<u64>,
}

impl Gauge {
    pub fn is_cooled(&self, now: u64) -> bool {
        matches!(self.cooled_until, Some(t) if t > now)
    }

    /// The first window at or over `ceiling_percent` that has not refilled
    /// yet, if any. A window whose reset time has passed is stale and counts
    /// as empty.
    pub fn full_window(&self, ceiling_percent: f64, now: u64) -> Option<&Window> {
        self.windows.iter().find(|w| {
            let refilled = matches!(w.resets_at, Some(t) if t <= now);
            !refilled && w.used_percent >= ceiling_percent
        })
    }
}

/// All known gauges. An account with no entry has an empty gauge, which means
/// "no information, assume room" (programs that report nothing rely on the
/// limit-hit signal instead).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Gauges {
    map: BTreeMap<AccountId, Gauge>,
}

impl Gauges {
    pub fn new() -> Self {
        Gauges::default()
    }

    pub fn get(&self, id: &AccountId) -> Option<&Gauge> {
        self.map.get(id)
    }

    pub fn set_windows(&mut self, id: &AccountId, windows: Vec<Window>) {
        self.map.entry(id.clone()).or_default().windows = windows;
    }

    /// Record that `id` hit its limit. The cooldown lasts until `resets_at`
    /// when that is in the future, otherwise `default_backoff_secs` from
    /// `now`. An existing longer cooldown is kept. Returns the new
    /// cooled-until second.
    pub fn on_limit_hit(
        &mut self,
        id: &AccountId,
        resets_at: Option<u64>,
        now: u64,
        default_backoff_secs: u64,
    ) -> u64 {
        let until = match resets_at {
            Some(t) if t > now => t,
            _ => now.saturating_add(default_backoff_secs),
        };
        let g = self.map.entry(id.clone()).or_default();
        let until = g.cooled_until.map_or(until, |old| old.max(until));
        g.cooled_until = Some(until);
        until
    }
}
