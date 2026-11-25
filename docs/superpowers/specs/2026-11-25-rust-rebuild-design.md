# HEB Job Alerts: Rust Rebuild Design

Date: 2026-11-25
Branch: `feature/rust-rebuild`

## Intent

The owner's brother is looking for an entry-level H-E-B job (curbie, bagger,
checker, customer service, part-time). Because of transportation he can only
work at three San Antonio stores: **49, 50 and 52**. The owner is certain of
these numbers. The stores are at **Potranco & SH 211**, **Culebra & SH 211**
and **Alamo Ranch Parkway**. Which number goes with which store isn't
recorded, and it doesn't matter, because the number is the primary key. Postings at those
stores are rare and fill fast, so he needs to hear about a new opening within
a couple of hours of it going up.

The owner also wants to learn Rust for their own job search. The rebuild is
in Rust, written entirely by Claude, with lots of comments explaining the
Rust concepts as they come up, so the owner can learn by reading and
tweaking it.

## Why a rebuild (what is wrong today)

The current Python script (`main.py`) has been silently broken. GitHub runs
show green, but they find zero jobs:

- It sends `internal=true`, which now returns
  `403 "Authentication required to access internal job requisitions"`.
  The owner is an employee, but we decided against using their login
  (cookies expire, and it ties their employee account to a bot). Public
  listings are enough.
- It joins every search term into one `keywords` query, and the API ANDs
  the words, so 6 or 14 terms return 0 results.
- It exits 0 either way, so nobody finds out.
- It walks back from page 10, but the results it needs all fit on 1 to 2 pages.
- It re-sends the same list every day, with no memory of what was sent.

## Goals

- Check every ~2 hours, around the clock, via GitHub Actions cron.
- Alert **only on new postings** (never re-send a job already emailed).
- Match on **store identity** (store number, with street names as backup),
  not loose keywords.
- Send **any** new opening at stores 49/50/52, except management roles.
  Entry-level roles go first and are labeled. Everything else follows in a
  second section.
- **Fail loudly**: if HEB's API errors out or returns obviously wrong data,
  the GitHub run fails (red X), so GitHub's failure email reaches the owner.
- Heavily commented, idiomatic Rust with unit tests a learner can read.

## Non-goals

- Internal (employee-only) postings / authenticated scraping.
- The owner's own job search (their profile is dropped).
- More than one recipient profile. Recipients come from a secret and may be
  a comma-separated list, but there is one set of match rules.
- SMS / push notifications. Email only (owner's decision).
- A web UI, a database, or any hosting beyond GitHub Actions.

## Anti-goals (would count as failure even if it "works")

- Runs that are green while nothing is actually being checked (the current bug).
- Re-sending the same jobs, or flooding him with every San Antonio HQ job.
- Missing a real opening at 49/50/52 because a filter was too clever.
- Personal email addresses committed to the public repo.

## Constraints

- Repo `HEB_Job_Notis` is **public**. Nothing personal goes in tracked files.
- API: `GET https://heb.jibeapply.com/api/jobs` (public, no `internal` param).
  Browser-like `user-agent` and `referer` headers. Response:
  `{ jobs: [{ data: {...} }], totalCount, count, ... }`. Useful `data` fields
  confirmed on 2026-11-25: `req_id`, `slug`, `title`, `location_name`
  (e.g. `SA11-Valley Hi Dr/Loop 410`), `city`, `state`, `street_address`,
  `postal_code`, `posted_date`, `employment_type` (`PART_TIME`/`FULL_TIME`),
  `tags1` (e.g. `["Part Time"]`), `tags2`, `categories`, `latitude`/`longitude`.
  Job link: `https://careers.heb.com/jobs/{slug}`.
- Store jobs show the store in two forms: `location_name` starts with
  `SA<n>` (e.g. `SA34-Montgomery/Walzem`), and the title often starts with
  `San Antonio <n> (<cross streets>)`. No postings exist at 49/50/52 today,
  so their exact labels are unconfirmed.
- Gmail SMTP with an app password (existing secrets `EMAIL_ID`, `EMAIL_PWORD`).
- GitHub's scheduled runs can start 10 to 30+ minutes late. That's acceptable.
- Rust is not installed locally yet. Install via `rustup` (stable) to build
  and test.

## Approaches considered

1. **GitHub Actions every 2h, `seen.json` committed back to the repo. CHOSEN.**
   Free, needs no machine of the owner's to be on, and the state is auditable
   in git history. The file holds only HEB job IDs, so it's safe in a public
   repo. Cost: automated commits (only when the file changes).
2. GitHub Actions, with state kept in the Actions cache. No commits, but
   cache eviction would cause mass re-sends or need extra logic. Rejected as
   untrustworthy.
3. A local systemd timer. Only runs while the owner's PC is on, and missing
   postings defeats the purpose. Rejected.

Language: Rust (owner's request; replaces Python entirely. The old code
stays in git history).

## Design

### Run flow (one invocation)

1. Load `config.toml` (tracked) and env secrets.
2. Fetch San Antonio-area listings from the public API, paging until a page
   returns fewer than `limit` jobs or the collected count reaches
   `totalCount`. Retry transient failures (network error, 5xx, 429) a couple
   of times with a short backoff.
3. **Sanity check**: if every request failed, or the San Antonio query
   returned 0 jobs in total (there are always 100+ HQ jobs), exit non-zero
   with a clear message. This is the guard against "green but broken".
4. Keep jobs that match a target store (rules below). Drop management roles.
5. Drop jobs whose `req_id` is already in `seen.json`.
6. If anything is left: send one email (HTML plus a plain-text part). Only
   after the send succeeds, add those IDs to `seen.json`. If the send fails,
   exit non-zero and leave `seen.json` untouched, so the next run retries.
7. Prune `seen.json`: drop IDs first seen more than 90 days ago that are no
   longer in the current listing.
8. The workflow commits `seen.json` only if it changed.

### Store matching

Store numbers are authoritative (confirmed by the owner). A job matches
store N (N in 49/50/52) if its `location_name` or title carries that store
number in any of the label forms seen in live data:

- `SA49-...`, `SA 49 ...`, and zero-padded forms like `SA049`
  (`location_name`, e.g. `SA11-Valley Hi Dr/Loop 410`, `SA02 CM-Broadway/Patterson`)
- `San Antonio 49 - ...` (`location_name`, e.g. `San Antonio 53 - IH10/Loop 1604 E`)
- `San Antonio 49 (...)` (title prefix, e.g. `San Antonio 34 (Montgomery/Walzem) ...`)

Compare numbers as integers after stripping leading zeros, with word
boundaries, so `SA5` or `SA490` never match 49.

Backup, used only when neither field carries any recognizable store number:
if the location, title or `street_address` mentions one of the configured
street pairs and the city is San Antonio, include the job, marked
**"check location"** in the email. Street pairs (both parts must appear,
case-insensitive): `Potranco` + `211`, `Culebra` + `211`, `Alamo Ranch`
(on its own). A job that names a *different* store number is never matched
by street. That's safe because the numbers are confirmed.

### Role classification

- **Excluded (management):** title contains any of `manager`, `director`,
  `supervisor`, `unit director`, `assistant director` (case-insensitive,
  list in config).
- **Entry-level (top section):** title contains any of `curbie`, `bagger`,
  `checker`, `cashier`, `customer service`, `service rep`, `csa`,
  `courtesy`, `store partner`, `stocker`, `estore`, or the job is part-time
  (`employment_type == PART_TIME` or `tags1` contains "Part Time").
  List in config.
- **Other (second section):** everything else at the target stores.

### Email

- To: `ALERT_TO` secret (comma-separated). From: `EMAIL_ID`.
- Subject: `HEB: N new opening(s) at your stores` (with "1 new opening" for one).
- Body: an "Entry-level" section, then "Other roles". Each row shows the
  title (linked), store, part/full time, posted date, and a "check location"
  tag where it applies. Clean inline-styled HTML with a plain-text fallback.
- No email when there's nothing new.

### Files

- `Cargo.toml`, `src/main.rs` (orchestration, CLI), plus modules such as
  `api.rs` (types and fetch), `matcher.rs` (store and role rules),
  `state.rs` (seen.json), `email.rs` (render and send), `config.rs`.
- `config.toml` (tracked): store numbers, street names, keyword lists,
  search location, page limit. No emails.
- `seen.json` (tracked): `{ "last_checked": "<ISO date>", "seen": { "<req_id>": "<first_seen ISO date>" } }`.
- Unit tests for matching, classification, dedupe and pruning, using small
  JSON fixtures shaped like real API responses.
- Removed: `main.py`, `requirements.txt`, `config.json` (it holds personal
  emails), and the Python bits of `.gitignore`. Add `target/`.
- README and CLAUDE.md rewritten for the Rust version.

### CLI

- `heb-alerts` runs the full flow (used by CI).
- `heb-alerts --dry-run` fetches and matches, then prints what *would* be
  sent. It doesn't email or write `seen.json`. This replaces the old
  interactive mode for local testing.

### GitHub Actions

- Cron `0 */2 * * *` plus `workflow_dispatch`.
- **Keep-alive**: GitHub disables scheduled workflows in public repos after
  60 days without repo activity, and quiet weeks are normal here. `seen.json`
  carries a top-level `last_checked` date. The binary updates it when it's
  more than 7 days old, so the bot commits at least weekly and the cron
  never gets switched off. (Claude's call.)
- `permissions: contents: write`. A `concurrency` group so runs never overlap.
- Steps: checkout, stable Rust toolchain, `Swatinem/rust-cache`,
  `cargo test`, `cargo run --release`, then commit and push `seen.json` if it
  changed (bot identity, message like `chore: update seen jobs`).
- Secrets: `EMAIL_ID`, `EMAIL_PWORD`, `ALERT_TO` (new). The old `JOB_*`
  vars are no longer used.

## Calls made by Claude (owner can override)

- Weekly keep-alive commit (see GitHub Actions) so the cron is never
  auto-disabled.

- First run with an empty `seen.json` emails whatever currently matches.
  There's nothing at 49/50/52 today, so this sends nothing, and later runs
  pick up real postings.
- Runs 24/7 (no overnight pause). Earlier notice beats quiet hours.
- No "still alive" heartbeat email. The sanity check plus the failing run
  covers breakage, and days of silence are normal.
- Personal emails already in git history stay there. Rewriting public
  history isn't worth it. They're just removed going forward.

## Left to the builder

- Crate choices (expected: `reqwest` blocking with rustls, `serde`,
  `serde_json`, `toml`, `lettre` with rustls, `chrono`, `regex`, `clap` or
  plain `std::env::args`, `anyhow`).
- Exact API query parameters for "San Antonio area" (verify `location`
  plus any radius param returns store jobs, not just HQ). If the location
  filter misses store postings, fall back to a full unfiltered scan and
  filter locally.
- Module layout, retry counts, and email styling details.
