# HEB Job Alerts Rust Rebuild: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (Native, chosen) to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Replace the silently broken Python script with a heavily commented Rust binary. It checks HEB's public job API every 2 hours and emails the owner's brother only *new* openings at stores 49, 50 and 52.

**Architecture:** One `heb-alerts` binary, split into focused modules: config, API fetch, matching, seen-state, email. GitHub Actions runs it on a cron and commits `seen.json` back to the repo when it changes. Pure logic (matching, state, rendering) is unit-tested against JSON fixtures shaped like live API data.

**Tech Stack:** Rust stable (edition 2021), `reqwest` (blocking, rustls), `serde`/`serde_json`, `toml`, `lettre` (smtp, rustls), `chrono`, `regex`, `anyhow`. No `clap`: plain `std::env::args` for the one `--dry-run` flag.

**Spec:** `docs/superpowers/specs/2026-11-25-rust-rebuild-design.md` (committed, 8e8dc0d, branch `feature/rust-rebuild`).

## Context

The GitHub runs show green, but they've found zero jobs for weeks:
- `internal=true` now returns a 403.
- The API ANDs every search term, so long term lists match nothing.
- The script exits 0 regardless.

The brother can only work at stores 49, 50 and 52, where postings are rare. Speed and dedupe matter more than volume. The owner wants to learn Rust from the result, so every module gets explanatory comments aimed at a Rust beginner (ownership, `Result`/`?`, traits, derive macros, iterators).

## Global Constraints

- API: `GET https://heb.jibeapply.com/api/jobs`. **Never** send `internal`. Send browser-like `user-agent`, `accept`, `origin` and `referer: https://heb.jibeapply.com/` headers (copy the values from the current `main.py` session headers).
- Job link: `https://careers.heb.com/jobs/{slug}`.
- Target stores: `[49, 50, 52]`. Street backup pairs: `["potranco","211"]`, `["culebra","211"]`, `["alamo ranch"]`.
- Exclude terms: `manager`, `director`, `supervisor`, `unit director`, `assistant director`.
- Entry-level terms: `curbie`, `bagger`, `checker`, `cashier`, `customer service`, `service rep`, `csa`, `courtesy`, `store partner`, `stocker`, `estore`, plus part-time (`employment_type == "PART_TIME"` or `tags1` contains "Part Time").
- Subject: `HEB: 1 new opening at your stores` / `HEB: N new openings at your stores`.
- Env: `EMAIL_ID`, `EMAIL_PWORD`, `ALERT_TO` (comma-separated). No personal data in tracked files (the repo is public).
- `seen.json` shape: `{ "last_checked": "YYYY-MM-DD", "seen": { "<req_id>": "YYYY-MM-DD" } }`. Prune entries older than 90 days that are absent from the current listing. Keep-alive: bump `last_checked` when it's more than 7 days old.
- Cron `0 */2 * * *` plus `workflow_dispatch`, `permissions: contents: write`, and a `concurrency` group.
- Commits: concise, **no Co-Authored-By lines** (the user's CLAUDE.md overrides the harness default).
- ASCII only, no emojis.

## Review Focus

1. **Store-number false positives:** `SA490`, `SA5`, `SA 4`, a title like `Pharmacy Tech 49 hrs`, or `San Antonio 50` appearing mid-title must not match. Only label-shaped numbers count. Tested in Task 3.
2. **Corrupt `seen.json`:** a missing file means a fresh state. A file that fails to parse must error out, not reset silently, because a reset would re-send everything. Tested in Task 4.
3. **HTML in titles:** titles containing `&`, `<` or `"` are escaped in the HTML email. Tested in Task 5.
4. **Messy `ALERT_TO`:** `" a@x.com, ,b@y.com "` yields 2 trimmed recipients. An empty value is an error. Tested in Task 6.
5. **Duplicate jobs across pages:** the same `req_id` on two pages is emailed once. Tested in Task 2.

---

### Task 1: Toolchain, scaffold, config

**Files:**
- Create: `Cargo.toml`, `config.toml`, `src/main.rs` (stub), `src/config.rs`
- Modify: `.gitignore` (add `target/`; leave the Python entries until Task 7)

**Interfaces:**
- Produces: `config::Config { search: Search, stores: Stores, roles: Roles }`, `Search { location: String, limit: u32, max_pages: u32 }`, `Stores { numbers: Vec<u32>, street_pairs: Vec<Vec<String>> }`, `Roles { exclude: Vec<String>, entry_level: Vec<String> }`, `Config::load(path: &Path) -> anyhow::Result<Config>`. All derive `Deserialize, Debug, Clone`.

- [ ] Install Rust: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y`, then `cargo --version` prints a version.
- [ ] `cargo init --name heb-alerts`, then add the dependencies (above) to `Cargo.toml`.
- [ ] Write `config.toml` with the Global Constraints values: `location = "San Antonio, TX"`, `limit = 100`, `max_pages = 10`.
- [ ] Failing test `config::tests::loads_repo_config`: loads `config.toml`, asserts `stores.numbers == [49,50,52]`, `street_pairs.len() == 3`, and that `roles.exclude` contains `"manager"`.
- [ ] Implement `Config::load` (read the file with `fs::read_to_string`, then `toml::from_str`, with `anyhow::Context` naming the path).
- [ ] `cargo test` passes. Commit `feat: scaffold Rust project and config`.

### Task 2: API types and fetching

**Files:** Create `src/api.rs`, `tests/fixtures/page_sa.json` (trimmed real response: include `SA11-Valley Hi Dr/Loop 410`, `San Antonio 53 - IH10/Loop 1604 E`, an HQ job, and a synthetic `SA49-Potranco/SH 211` curbie job)

**Interfaces:**
- Consumes: `config::Search`.
- Produces: `api::Job { req_id: String, slug: String, title: String, location_name: Option<String>, city: Option<String>, street_address: Option<String>, posted_date: Option<String>, employment_type: Option<String>, tags1: Option<Vec<String>> }` with `Job::link(&self) -> String` and `Job::is_part_time(&self) -> bool`. `api::Page { jobs: Vec<Job>, total_count: u64 }`, `api::parse_page(body: &str) -> anyhow::Result<Page>` (the API nests fields under `jobs[].data`). `api::collect_pages(fetch: impl FnMut(u32) -> anyhow::Result<String>, limit: u32, max_pages: u32) -> anyhow::Result<Vec<Job>>`. `api::fetch_all(search: &Search) -> anyhow::Result<Vec<Job>>` (builds the reqwest client and calls `collect_pages`).

- [ ] Failing tests:
  - `parse_page_reads_fixture`: the job count matches the fixture, and `job.link()` == `https://careers.heb.com/jobs/<slug>`.
  - `collect_stops_on_short_page`: a fake fetch returns a full page and then a short one, and is called 2 times.
  - `collect_dedupes_req_id`: the same job on two pages yields 1.
  - `collect_errors_when_all_pages_fail`.
  - `collect_errors_when_total_zero`: the sanity guard.
  - `collect_tolerates_one_failed_page`: page 2 errors, page 1 jobs are returned, and a warning is printed.
- [ ] Implement. `collect_pages` loops pages 1..=max_pages and stops when `jobs.len() < limit` or the collected count reaches `total_count`. `fetch_all` retries each page up to 3 times on network error, 5xx or 429, with backoff of 2s and 4s. A non-200 that isn't retryable is an `Err` for that page. Params: `location`, `page`, `limit`, `sortBy=posted_date` if accepted, else omitted. Log `page=N status=… jobs=…` lines like the old script.
- [ ] Live check: a small `#[ignore]` test `live_fetch_san_antonio` (`cargo test -- --ignored live_fetch`) asserts more than 50 jobs and at least one job whose `location_name` starts with `SA`. If store jobs are missing, switch `fetch_all` to an unfiltered scan per the spec fallback and note it in the commit.
- [ ] `cargo test` passes. Commit `feat: fetch HEB public job listings`.

### Task 3: Store matching and role classification

**Files:** Create `src/matcher.rs`

**Interfaces:**
- Consumes: `api::Job`, `config::{Config, Stores, Roles}`.
- Produces: `matcher::store_number(text: &str) -> Option<u32>`, `enum StoreMatch { Number(u32), Street }`, `matcher::match_store(job: &Job, stores: &Stores) -> Option<StoreMatch>`, `enum Role { EntryLevel, Other }`, `matcher::classify(job: &Job, roles: &Roles) -> Option<Role>` (`None` = excluded management), `struct Hit { job: Job, store: StoreMatch, role: Role }`, `matcher::select(jobs: &[Job], cfg: &Config) -> Vec<Hit>` (sorted EntryLevel first, then newest `posted_date`).

- [ ] Failing tests for `store_number`. `Some(n)` for `"SA49-Potranco"` (49), `"SA 50 Culebra"` (50), `"SA049-x"` (49), `"San Antonio 52 - Alamo Ranch"` (52), `"San Antonio 49 (Potranco/211) Curbie - Part time"` (49), `"SA02 CM-Broadway/Patterson"` (2). `None` for `"SA490"`→490 (i.e. `!= 49`), `"Pharmacy Tech 49 hrs"`, `"Headquarters-Arsenal"`, and `"Senior Analyst San Antonio 50"` (not a prefix).
- [ ] Failing tests for `match_store`. A number match on 49/50/52 gives `Number`. A different number (`SA11`) with `Culebra/211` in the text gives `None`. No number, with `Culebra & SH 211` and city San Antonio, gives `Street`. No number with `Culebra/Tezel` gives `None` (pair incomplete). No number with `Alamo Ranch Pkwy` gives `Street`.
- [ ] Failing tests for `classify`: `"Store Manager"` gives `None`; `"Curbie - Part time"` gives `EntryLevel`; a `PART_TIME` pharmacy tech gives `EntryLevel`; a full-time `"Pharmacy Technician"` gives `Other`; `"CSA"` matches on word boundaries but `"Physician"` doesn't match `csa`.
- [ ] Implement. `store_number` uses anchored regexes compiled once (`std::sync::OnceLock`): `^SA\s*0*(\d{1,3})\b` and `^San Antonio\s+0*(\d{1,3})\b`, both case-insensitive. `match_store` checks `location_name` then `title`. Street backup applies only when neither has any store number, and matches lowercase substrings across `location_name`, `title` and `street_address`. Keyword checks use `\b` word-boundary regexes.
- [ ] `cargo test` passes. Commit `feat: match target stores and classify roles`.

### Task 4: Seen-state

**Files:** Create `src/state.rs`, plus an initial `seen.json` (`{"last_checked":"<today>","seen":{}}`)

**Interfaces:**
- Consumes: `matcher::Hit`.
- Produces: `state::SeenState { last_checked: NaiveDate, seen: BTreeMap<String, NaiveDate> }` (serde, BTreeMap for stable diffs). `SeenState::load(path: &Path, today: NaiveDate) -> anyhow::Result<SeenState>`, `save(&self, path: &Path) -> anyhow::Result<()>` (pretty JSON plus a trailing newline), `filter_new(&self, hits: Vec<Hit>) -> Vec<Hit>`, `mark_sent(&mut self, ids: impl IntoIterator<Item = String>, today: NaiveDate)`, `prune(&mut self, current_ids: &HashSet<String>, today: NaiveDate)`, and `touch_keepalive(&mut self, today: NaiveDate) -> bool`.

- [ ] Failing tests:
  - `missing_file_is_fresh_state`.
  - `corrupt_file_errors`: Review Focus 2.
  - `filter_new_drops_seen`.
  - `mark_sent_keeps_first_seen_date`: re-marking doesn't overwrite.
  - `prune_drops_old_absent_only`: 91 days old and absent is dropped. 91 days old and present is kept. 10 days old and absent is kept.
  - `keepalive_bumps_after_7_days`: 8 days is bumped and returns true. 3 days is unchanged and returns false.
  - `save_load_roundtrip`.
- [ ] Implement.
- [ ] `cargo test` passes. Commit `feat: track already-sent jobs in seen.json`.

### Task 5: Email rendering and sending

**Files:** Create `src/email.rs`

**Interfaces:**
- Consumes: `matcher::{Hit, Role, StoreMatch}`.
- Produces: `email::subject(n: usize) -> String`, `email::render(hits: &[Hit]) -> (String /*html*/, String /*text*/)`, `email::Creds { user: String, password: String }`, and `email::send(creds: &Creds, to: &[String], subject: &str, html: &str, text: &str) -> anyhow::Result<()>`.

- [ ] Failing tests:
  - `subject_singular_plural`: copy exactly as in Global Constraints.
  - `render_sections_in_order`: the "Entry-level" heading comes before "Other roles", and an empty section is omitted.
  - `render_marks_street_matches`: `StoreMatch::Street` rows contain "check location".
  - `render_escapes_html`: title `A & B <x>` gives `A &amp; B &lt;x&gt;` in the HTML and the raw text in the plain-text part.
  - Each row shows the link, the store label, Part/Full time, and the `posted_date` (YYYY-MM-DD).
- [ ] Implement. Inline-styled HTML table (HEB red `#E1251B` header accent, readable on mobile). Add a small `escape_html` helper. `send` uses lettre's `SmtpTransport::starttls_relay("smtp.gmail.com")` with port 587 and a multipart alternative (text and HTML), and returns errors rather than printing and continuing (fixing the old swallow).
- [ ] `cargo test` passes. Commit `feat: render and send alert emails`.

### Task 6: Orchestration and `--dry-run`

**Files:** Modify `src/main.rs`

**Interfaces:**
- Consumes: everything above.
- Produces: `fn parse_recipients(raw: &str) -> anyhow::Result<Vec<String>>` and `fn run(dry_run: bool) -> anyhow::Result<()>`. `main` returns `anyhow::Result<()>`, so any error makes a non-zero exit (a red CI run).

- [ ] Failing tests: `parse_recipients_trims_and_skips_empty` (Review Focus 4) and `parse_recipients_empty_errors`.
- [ ] Implement `run`. Flow: load config, load `seen.json` with today's date (`chrono::Local` in the `America/Chicago` zone is unnecessary, so use `Utc::now().date_naive()`), `fetch_all`, `select`, `filter_new`. In dry-run, print the would-send list (and the rendered text), then return without email or save. Otherwise, if there are new hits, read `EMAIL_ID`/`EMAIL_PWORD`/`ALERT_TO`, send, and only then `mark_sent`. Finally run `prune` with the current IDs, then `touch_keepalive`, then `save`. Print a one-line summary (`fetched=N matched=M new=K sent=yes/no`).
- [ ] Live verification: `cargo run -- --dry-run` exits 0, prints a fetch count over 50 and `matched=0` (nothing at 49/50/52 today). Then temporarily point `config.toml` numbers at `[11, 34]`, rerun, and confirm the pharmacy-tech jobs show up with the right sections. Revert the config.
- [ ] Live email check (the owner's inbox, with their consent at execution time): with `.env` providing `EMAIL_ID`/`EMAIL_PWORD`, `ALERT_TO=<owner>` and stores `[11, 34]`, run against a scratch `seen.json` copy and confirm one email arrives. Revert the config and discard the scratch state.
- [ ] Commit `feat: wire up alert run with dry-run mode`.

### Task 7: Workflow, cleanup, docs

**Files:**
- Modify: `.github/workflows/actions.yml`, `.gitignore`, `README.md`, `CLAUDE.md`
- Delete: `main.py`, `requirements.txt`, `config.json`, `__pycache__/` (if tracked)

- [ ] Rewrite the workflow: name `heb job alerts`; cron `0 */2 * * *` plus `workflow_dispatch`; `permissions: contents: write`; `concurrency: { group: heb-alerts, cancel-in-progress: false }`. Steps: `actions/checkout@v4`, `dtolnay/rust-toolchain@stable`, `Swatinem/rust-cache@v2`, `cargo test --release`, `cargo run --release` (env `EMAIL_ID`, `EMAIL_PWORD`, `ALERT_TO` from secrets), then a commit step that runs `git diff --quiet seen.json || (git config user.name github-actions[bot]; git config user.email 41898282+github-actions[bot]@users.noreply.github.com; git add seen.json; git commit -m "chore: update seen jobs"; git push)`.
- [ ] Validate the YAML: `python3 -c "import yaml,sys;yaml.safe_load(open('.github/workflows/actions.yml'))"` succeeds.
- [ ] Remove the Python files and the Python-only `.gitignore` noise (keep `.env`, `venv/` and `target/` ignored).
- [ ] Rewrite `README.md` for the Rust version (what it does, setup, secrets including the new `ALERT_TO`, dry-run, how matching works). Rewrite `CLAUDE.md` to match (it still documents the Python architecture).
- [ ] `cargo build --release && cargo test` passes, and `cargo clippy -- -D warnings` is clean.
- [ ] Commit `chore: replace Python with Rust; 2-hourly workflow`.
- [ ] Push the branch and open a PR to `main` (per CLAUDE.md). Tell the owner to add the `ALERT_TO` secret (`gh secret set ALERT_TO`, with the value entered by them) before merging, then trigger `workflow_dispatch` once after merge and confirm a green run.

## Verification (end-to-end)

- `cargo test` passes all unit tests. `cargo test -- --ignored` passes the live API test.
- `cargo run -- --dry-run` shows a real fetch count and correct matching (store override check in Task 6).
- A test email reaches the owner's inbox (Task 6).
- After merge and the `ALERT_TO` secret: a manual workflow run is green, and a `seen.json` keep-alive or no-op behaves as expected.
