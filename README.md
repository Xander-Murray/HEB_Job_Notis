# HEB Job Alerts

A small Rust program that watches H-E-B's public job listings and emails you
when a **new** opening appears at specific stores. GitHub Actions runs it every
2 hours. You only get an email when something new is posted, and each job is
sent once.

The code is commented heavily to double as a Rust learning project.

## How it works

1. **Fetch:** pull every San Antonio-area posting from HEB's public API
   (`heb.jibeapply.com/api/jobs`). If any page fails after retries, or the API
   reports zero jobs, the run fails loudly instead of quietly sending nothing.
   Real runs also check the Gmail login and the secrets before fetching.
2. **Match stores:** keep jobs whose store label carries one of the configured
   store numbers (`SA49-...`, `San Antonio 49 (...)`, `SA049`, ...). Postings
   with no store number at all can still match by street, for example
   `Potranco` + `211`. Those are marked **check location** in the email.
   Store postings that cover several locations (HEB only names the primary
   store) are included when they list San Antonio, marked **multi-store
   posting, check location**.
3. **Classify:** management roles are dropped. Entry-level roles (curbie,
   bagger, checker, CSA, part-time, ...) go first. Everything else goes in
   "Other roles".
4. **Dedupe:** skip anything already in `seen.json`.
5. **Email:** send one HTML email (with a plain-text fallback) through Gmail.
   Jobs are added to `seen.json` only *after* the send succeeds.
6. **Commit:** the workflow commits `seen.json` back to the repo whenever it
   changes. A weekly keep-alive bump stops GitHub from disabling the schedule
   on quiet repos.

## Layout

| File | What it does |
|---|---|
| `src/main.rs` | CLI and the run flow |
| `src/api.rs` | API types, paging, retries, sanity checks |
| `src/matcher.rs` | Store matching and role classification |
| `src/state.rs` | `seen.json` load/save, dedupe, prune, keep-alive |
| `src/email.rs` | Email rendering and Gmail SMTP |
| `src/config.rs` | Typed loading of `config.toml` |
| `config.toml` | Stores, street pairs, keyword lists (no personal data) |
| `seen.json` | Jobs already emailed (HEB job ids and dates only) |

## Setup

Install Rust (https://rustup.rs), then:

```bash
cargo test                          # unit tests
cargo test -- --ignored live_fetch  # hits the real HEB API
cargo run -- --dry-run              # fetch and match; prints, no email, no state change
```

For a real local run, create `.env` and wrap values containing spaces in
double quotes:

```bash
EMAIL_ID=you@gmail.com
EMAIL_PWORD="abcd efgh ijkl mnop"
ALERT_TO=someone@gmail.com,another@gmail.com
```

`EMAIL_PWORD` is a Gmail **app password**
(https://myaccount.google.com/apppasswords; it needs 2-step verification
turned on), not your normal password.

## GitHub Actions

`.github/workflows/actions.yml` runs every 2 hours (`0 */2 * * *`, UTC), and
can also be started by hand from the Actions tab.

Repository **secrets** (Settings > Secrets and variables > Actions):

| Secret | Value |
|---|---|
| `EMAIL_ID` | Gmail address that sends the alerts |
| `EMAIL_PWORD` | Its app password |
| `ALERT_TO` | Comma-separated recipient addresses |

A failed run (bad password, API down, missing secret) shows a red X, and
GitHub emails the repo owner.

## Changing what it watches

Edit `config.toml`. The repo is public, so keep email addresses in the
`ALERT_TO` secret, never in this file.
