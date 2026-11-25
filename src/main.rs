//! HEB Job Alerts: emails new openings at a few specific H-E-B stores.
//!
//! Run flow (see `run`): fetch listings -> match our stores -> drop jobs we
//! already emailed -> email the rest -> remember them in `seen.json`.
//!
//! Usage:
//!   heb-alerts            full run (what GitHub Actions does)
//!   heb-alerts --dry-run  fetch and match, print what would be sent;
//!                         no email, no changes to seen.json

// Rust lesson: `mod foo;` tells the compiler to include `src/foo.rs` as a
// module. Each module is its own namespace; `pub` items are visible outside.
mod api;
mod config;
mod email;
mod matcher;
mod state;

use std::{collections::HashSet, env, path::Path};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use lettre::message::Mailbox;

use crate::{config::Config, email::Creds, state::SeenState};

const CONFIG_PATH: &str = "config.toml";
const STATE_PATH: &str = "seen.json";

/// Split a comma-separated recipient list, trimming spaces and skipping
/// empty entries.
///
/// Each address is validated now. Errors say *which* entry is bad (`#2`)
/// but never print the address: this repo's Actions logs are public.
fn parse_recipients(raw: &str) -> Result<Vec<Mailbox>> {
    let entries: Vec<&str> = raw.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    if entries.is_empty() {
        bail!("ALERT_TO has no recipient addresses");
    }
    entries
        .iter()
        .enumerate()
        // Collecting an iterator of `Result`s into `Result<Vec<_>>` stops at
        // the first error, a handy Rust idiom.
        .map(|(i, addr)| {
            addr.parse::<Mailbox>()
                .map_err(|_| anyhow::anyhow!("ALERT_TO entry #{} is not a valid email address", i + 1))
        })
        .collect()
}

/// Reject a blank secret. In GitHub Actions an unset secret arrives as an
/// empty string, not as a missing variable.
fn non_blank(name: &str, value: String) -> Result<String> {
    if value.trim().is_empty() {
        bail!("environment variable {name} is empty");
    }
    Ok(value)
}

/// Read a required environment variable with a helpful error.
fn require_env(name: &str) -> Result<String> {
    let value = env::var(name).with_context(|| format!("environment variable {name} must be set"))?;
    non_blank(name, value)
}

/// Email settings, read up front on real runs so a missing secret fails the
/// run immediately, even on quiet days with nothing to send.
struct Mail {
    creds: Creds,
    to: Vec<Mailbox>,
}

fn run(dry_run: bool) -> Result<()> {
    let cfg = Config::load(Path::new(CONFIG_PATH))?;
    let mail = if dry_run {
        None
    } else {
        let creds = Creds { user: require_env("EMAIL_ID")?, password: require_env("EMAIL_PWORD")? };
        email::parse_sender(&creds.user)?;
        let to = parse_recipients(&require_env("ALERT_TO")?)?;
        email::check_login(&creds)?;
        Some(Mail { creds, to })
    };

    let today = Utc::now().date_naive();
    let state_path = Path::new(STATE_PATH);
    let mut state = SeenState::load(state_path, today)?;

    let jobs = api::fetch_all(&cfg.search)?;
    let hits = matcher::select(&jobs, &cfg);
    let matched = hits.len();
    let new_hits = state.filter_new(hits);

    // Rust lesson: `let ... else` destructures or runs the `else` branch,
    // which must leave the function. Here: no `Mail` means dry run.
    let Some(mail) = mail else {
        println!("dry run: fetched={} matched={matched} new={}", jobs.len(), new_hits.len());
        if !new_hits.is_empty() {
            let (_, text) = email::render(&new_hits);
            println!("--- would send: {} ---\n{text}", email::subject(new_hits.len()));
        }
        return Ok(());
    };

    let new_count = new_hits.len();
    if new_count > 0 {
        let (html, text) = email::render(&new_hits);
        email::send(&mail.creds, &mail.to, &email::subject(new_hits.len()), &html, &text)?;
        // Only mark jobs as seen *after* the email went out. If sending
        // failed we returned above (`?`), so the next run retries them.
        state.mark_sent(new_hits.into_iter().map(|h| h.job.req_id), today);
    }

    let current_ids: HashSet<String> = jobs.iter().map(|j| j.req_id.clone()).collect();
    state.prune(&current_ids, today);
    state.touch_keepalive(today);
    state.save(state_path)?;

    println!("fetched={} matched={matched} new_sent={new_count}", jobs.len());
    Ok(())
}

fn main() -> Result<()> {
    // Load a local `.env` file if there is one (handy when running on your
    // own machine). In GitHub Actions there's no `.env`; secrets arrive as
    // real environment variables instead, so "not found" is fine.
    if let Err(err) = dotenvy::dotenv()
        && !err.not_found()
    {
        // Deliberately *not* including `err` itself: the parser's message
        // quotes the offending line, which could print a password.
        bail!(".env could not be parsed; values containing spaces must be wrapped in double quotes");
    }

    let args: Vec<String> = env::args().skip(1).collect();
    let dry_run = match args.as_slice() {
        [] => false,
        [flag] if flag == "--dry-run" => true,
        _ => bail!("usage: heb-alerts [--dry-run]"),
    };

    // Returning an `Err` from `main` prints it and exits with code 1, which
    // turns the GitHub Actions run red. That's how failures get noticed.
    run(dry_run)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_recipients_trims_and_skips_empty() {
        let got = parse_recipients(" a@x.com, ,b@y.com ").unwrap();
        let addrs: Vec<String> = got.iter().map(|m| m.email.to_string()).collect();
        assert_eq!(addrs, ["a@x.com", "b@y.com"]);
    }

    #[test]
    fn non_blank_rejects_empty_and_whitespace() {
        assert!(non_blank("X", String::new()).is_err());
        assert!(non_blank("X", "   ".into()).is_err());
        assert_eq!(non_blank("X", "v".into()).unwrap(), "v");
    }

    #[test]
    fn parse_recipients_rejects_invalid_address_without_echoing_it() {
        let err = parse_recipients("a@x.com,not-an-email").unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("#2"), "{msg}");
        // The repo's Actions logs are public: never print the address itself.
        assert!(!msg.contains("not-an-email"), "{msg}");
    }

    #[test]
    fn parse_recipients_empty_errors() {
        assert!(parse_recipients("").is_err());
        assert!(parse_recipients(" , ").is_err());
    }
}
