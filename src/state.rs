//! `seen.json`: the bot's memory of which jobs it has already emailed.
//!
//! The file is committed back to the repo by GitHub Actions after each run, so
//! it's also a readable history of what was sent and when:
//!
//! ```json
//! { "last_checked": "2026-11-25", "seen": { "250480": "2026-11-25" } }
//! ```

use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::matcher::Hit;

/// Forget a sent job this many days after first seeing it, but only once it's
/// also gone from HEB's listings (so a still-open job is never re-sent).
const PRUNE_AFTER_DAYS: i64 = 90;

/// Bump `last_checked` once it's older than this. GitHub turns off scheduled
/// workflows in public repos after 60 days with no commits, and quiet weeks
/// are normal here, so this guarantees at least one commit per week.
const KEEPALIVE_DAYS: i64 = 7;

/// Rust lesson: `BTreeMap` is a map that keeps its keys sorted. We use it
/// instead of `HashMap` so `seen.json` is written in the same order every
/// time, which keeps git diffs small and readable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeenState {
    pub last_checked: NaiveDate,
    /// Job `req_id` -> the date we first emailed it.
    pub seen: BTreeMap<String, NaiveDate>,
}

impl SeenState {
    /// Load the state file. A missing file means "first run": start empty.
    /// A file that exists but won't parse is an error. Silently starting
    /// fresh would re-send every job.
    pub fn load(path: &Path, today: NaiveDate) -> Result<SeenState> {
        if !path.exists() {
            return Ok(SeenState { last_checked: today, seen: BTreeMap::new() });
        }
        let text = fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| {
            format!("{} is not valid; fix or delete it by hand", path.display())
        })
    }

    /// Write the state as pretty JSON with a trailing newline.
    pub fn save(&self, path: &Path) -> Result<()> {
        let mut text = serde_json::to_string_pretty(self)?;
        text.push('\n');
        fs::write(path, text).with_context(|| format!("writing {}", path.display()))
    }

    /// Keep only hits we haven't emailed before.
    pub fn filter_new(&self, hits: Vec<Hit>) -> Vec<Hit> {
        // Rust lesson: taking `hits: Vec<Hit>` (not `&Vec<Hit>`) means the
        // caller hands over ownership, so we can move the hits we keep into
        // the result without cloning them.
        hits.into_iter()
            .filter(|h| !self.seen.contains_key(&h.job.req_id))
            .collect()
    }

    /// Record jobs as sent. A job already recorded keeps its original date.
    pub fn mark_sent(&mut self, ids: impl IntoIterator<Item = String>, today: NaiveDate) {
        for id in ids {
            // `entry(..).or_insert(..)` only inserts when the key is missing.
            self.seen.entry(id).or_insert(today);
        }
    }

    /// Drop entries that are both old and no longer listed by HEB.
    pub fn prune(&mut self, current_ids: &HashSet<String>, today: NaiveDate) {
        // `retain` keeps entries where the closure returns true.
        self.seen.retain(|id, first_seen| {
            let age_days = (today - *first_seen).num_days();
            age_days <= PRUNE_AFTER_DAYS || current_ids.contains(id)
        });
    }

    /// Bump `last_checked` if it's stale. Returns true if it changed.
    pub fn touch_keepalive(&mut self, today: NaiveDate) -> bool {
        if (today - self.last_checked).num_days() > KEEPALIVE_DAYS {
            self.last_checked = today;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matcher::{Role, StoreMatch, tests::job};

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    /// A unique scratch file path per test (no extra crates needed).
    fn temp_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("heb-alerts-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    fn hit(id: &str) -> Hit {
        let mut j = job("Curbie", Some("SA49-Potranco"));
        j.req_id = id.into();
        Hit { job: j, store: StoreMatch::Number(49), role: Role::EntryLevel }
    }

    fn state(entries: &[(&str, &str)], last_checked: &str) -> SeenState {
        SeenState {
            last_checked: d(last_checked),
            seen: entries.iter().map(|(k, v)| (k.to_string(), d(v))).collect(),
        }
    }

    #[test]
    fn missing_file_is_fresh_state() {
        let s = SeenState::load(&temp_path("does-not-exist.json"), d("2026-11-25")).unwrap();
        assert_eq!(s, state(&[], "2026-11-25"));
    }

    #[test]
    fn corrupt_file_errors() {
        let path = temp_path("corrupt.json");
        fs::write(&path, "{ not json").unwrap();
        assert!(SeenState::load(&path, d("2026-11-25")).is_err());
    }

    #[test]
    fn save_load_roundtrip() {
        let path = temp_path("roundtrip.json");
        let s = state(&[("1", "2026-11-01"), ("2", "2026-11-02")], "2026-11-20");
        s.save(&path).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.ends_with('\n'));
        assert_eq!(SeenState::load(&path, d("2026-12-31")).unwrap(), s);
    }

    #[test]
    fn filter_new_drops_seen() {
        let s = state(&[("1", "2026-11-01")], "2026-11-25");
        let new = s.filter_new(vec![hit("1"), hit("2")]);
        let ids: Vec<&str> = new.iter().map(|h| h.job.req_id.as_str()).collect();
        assert_eq!(ids, ["2"]);
    }

    #[test]
    fn mark_sent_keeps_first_seen_date() {
        let mut s = state(&[("1", "2026-11-01")], "2026-11-25");
        s.mark_sent(["1".to_string(), "2".to_string()], d("2026-11-25"));
        assert_eq!(s.seen["1"], d("2026-11-01"));
        assert_eq!(s.seen["2"], d("2026-11-25"));
    }

    #[test]
    fn prune_drops_old_absent_only() {
        let today = d("2026-11-25");
        let mut s = state(
            &[
                ("old_absent", "2026-08-26"),  // 91 days ago, gone from listings
                ("old_present", "2026-08-26"), // 91 days ago, still listed
                ("new_absent", "2026-11-15"),  // 10 days ago, gone
            ],
            "2026-11-25",
        );
        let current: HashSet<String> = ["old_present".to_string()].into();
        s.prune(&current, today);
        let keys: Vec<&str> = s.seen.keys().map(String::as_str).collect();
        assert_eq!(keys, ["new_absent", "old_present"]);
    }

    #[test]
    fn keepalive_bumps_after_7_days() {
        let mut s = state(&[], "2026-11-17");
        assert!(s.touch_keepalive(d("2026-11-25"))); // 8 days
        assert_eq!(s.last_checked, d("2026-11-25"));

        let mut s = state(&[], "2026-11-22");
        assert!(!s.touch_keepalive(d("2026-11-25"))); // 3 days
        assert_eq!(s.last_checked, d("2026-11-22"));
    }
}
