//! Talking to HEB's public job-search API.
//!
//! The API lives at `https://heb.jibeapply.com/api/jobs` and returns JSON like
//! `{ "jobs": [ { "data": { "title": ..., ... } } ], "totalCount": 112 }`.
//!
//! This module is split in two on purpose:
//! - `parse_page` / `collect_pages` are *pure* logic (no network), so unit
//!   tests can feed them fake pages.
//! - `fetch_all` is the thin network layer that plugs real HTTP into them.

use std::{collections::HashSet, thread, time::Duration};

use anyhow::{Context, Result, anyhow, bail};
use reqwest::blocking::Client;
use serde::Deserialize;

use crate::config::Search;

const API_URL: &str = "https://heb.jibeapply.com/api/jobs";

/// One job posting, with just the fields we use.
///
/// Rust lesson: `Option<String>` means "maybe a string, maybe nothing". HEB
/// sometimes sends `null` or leaves a field out, and `Option` forces every
/// caller to handle that case -- you can't accidentally call `.to_lowercase()`
/// on a missing value the way Python lets you call it on `None` and crash.
/// `#[serde(default)]` says "if the field is missing, use `None`".
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Job {
    pub req_id: String,
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub location_name: Option<String>,
    #[serde(default)]
    pub city: Option<String>,
    #[serde(default)]
    pub street_address: Option<String>,
    #[serde(default)]
    pub posted_date: Option<String>,
    #[serde(default)]
    pub employment_type: Option<String>,
    #[serde(default)]
    pub tags1: Option<Vec<String>>,
}

impl Job {
    /// Public link to the posting on HEB's careers site.
    pub fn link(&self) -> String {
        format!("https://careers.heb.com/jobs/{}", self.slug)
    }

    /// True if HEB marks the job part-time in either of the two places it
    /// records that.
    pub fn is_part_time(&self) -> bool {
        // Rust lesson: `as_deref()` turns `Option<String>` into `Option<&str>`
        // so we can compare against a string literal without copying.
        let by_type = self.employment_type.as_deref() == Some("PART_TIME");
        let by_tag = self
            .tags1
            .iter() // iterate over the Option (zero or one Vec)...
            .flatten() // ...then over the strings inside the Vec
            .any(|tag| tag.eq_ignore_ascii_case("part time"));
        by_type || by_tag
    }
}

/// One page of results.
#[derive(Debug)]
pub struct Page {
    pub jobs: Vec<Job>,
    pub total_count: u64,
}

// The raw JSON shape. Each job's fields are nested under a `data` key, so we
// deserialize into these private wrapper structs and then unwrap them.
#[derive(Deserialize)]
struct RawPage {
    // No `#[serde(default)]` here: an error response like
    // `{"error": "Authentication required..."}` has no `jobs` key, and we
    // *want* that to fail loudly instead of looking like "zero jobs".
    jobs: Vec<RawJob>,
    #[serde(rename = "totalCount", default)]
    total_count: u64,
}

#[derive(Deserialize)]
struct RawJob {
    data: Job,
}

/// Parse one API response body.
pub fn parse_page(body: &str) -> Result<Page> {
    let raw: RawPage = serde_json::from_str(body).with_context(|| {
        // Show the start of the body so a changed API is easy to diagnose.
        let preview: String = body.chars().take(200).collect();
        format!("unexpected API response: {preview}")
    })?;
    Ok(Page {
        // `into_iter().map(...)` consumes the Vec and builds a new one,
        // moving each `Job` out of its wrapper without copying it.
        jobs: raw.jobs.into_iter().map(|r| r.data).collect(),
        total_count: raw.total_count,
    })
}

/// Walk through result pages using `fetch` to get each page's raw body.
///
/// Rust lesson: `impl FnMut(u32) -> Result<String>` means "any function or
/// closure that takes a page number and returns a body". Real code passes an
/// HTTP call; tests pass a closure that returns canned JSON.
///
/// Rules:
/// - stop after a short page, or once we've seen `totalCount` jobs
/// - a single failed page is logged and skipped (the next run, 2 hours later,
///   will catch anything it hid)
/// - if *every* page failed, or the API says there are zero jobs, that's an
///   error: San Antonio always has 100+ postings, so zero means something is
///   broken (the exact bug the old Python script hid)
/// - the same job appearing on two pages is kept once
pub fn collect_pages(
    mut fetch: impl FnMut(u32) -> Result<String>,
    limit: u32,
    max_pages: u32,
) -> Result<Vec<Job>> {
    let mut jobs = Vec::new();
    let mut seen_ids = HashSet::new();
    let mut ok_pages = 0;
    let mut total_count = 0;

    for page_no in 1..=max_pages {
        // `and_then` chains two fallible steps: fetch, then parse.
        let page = match fetch(page_no).and_then(|body| parse_page(&body)) {
            Ok(page) => page,
            Err(err) => {
                eprintln!("warning: page={page_no} failed: {err:#}");
                continue;
            }
        };
        ok_pages += 1;
        total_count = total_count.max(page.total_count);
        let page_len = page.jobs.len();
        println!("page={page_no} jobs={page_len} total={}", page.total_count);

        for job in page.jobs {
            // `insert` returns false if the id was already there.
            if seen_ids.insert(job.req_id.clone()) {
                jobs.push(job);
            }
        }

        if page_len < limit as usize || jobs.len() as u64 >= total_count {
            break;
        }
    }

    if ok_pages == 0 {
        bail!("every request to the HEB jobs API failed");
    }
    if total_count == 0 || jobs.is_empty() {
        bail!("HEB jobs API returned zero jobs; the API or query is probably broken");
    }
    Ok(jobs)
}

/// Fetch every San Antonio-area job from the live API.
pub fn fetch_all(search: &Search) -> Result<Vec<Job>> {
    let client = build_client()?;
    collect_pages(
        |page_no| fetch_page(&client, search, page_no),
        search.limit,
        search.max_pages,
    )
}

fn build_client() -> Result<Client> {
    use reqwest::header::{HeaderMap, HeaderValue};

    // HEB's API expects browser-like headers (same values the old script used).
    let mut headers = HeaderMap::new();
    headers.insert("accept", HeaderValue::from_static("application/json, text/plain, */*"));
    headers.insert("accept-language", HeaderValue::from_static("en-US,en;q=0.9"));
    headers.insert(
        "user-agent",
        HeaderValue::from_static(
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36",
        ),
    );
    headers.insert("origin", HeaderValue::from_static("https://heb.jibeapply.com"));
    headers.insert("referer", HeaderValue::from_static("https://heb.jibeapply.com/"));

    Client::builder()
        .default_headers(headers)
        .timeout(Duration::from_secs(20))
        .build()
        .context("building HTTP client")
}

/// GET one page, retrying up to 3 attempts on network errors, 429 and 5xx.
fn fetch_page(client: &Client, search: &Search, page_no: u32) -> Result<String> {
    // Note: we deliberately never send `internal=true`. It now requires an
    // employee login and returns 403.
    let page = page_no.to_string();
    let limit = search.limit.to_string();
    let params = [
        ("location", search.location.as_str()),
        ("page", page.as_str()),
        ("limit", limit.as_str()),
    ];

    let backoffs = [Duration::from_secs(2), Duration::from_secs(4)];
    let mut attempt = 0;
    loop {
        let result = client.get(API_URL).query(&params).send();
        // Decide whether this attempt is worth retrying.
        let retry_reason = match result {
            Ok(resp) => {
                let status = resp.status();
                println!("page={page_no} status={}", status.as_u16());
                if status.is_success() {
                    return resp.text().context("reading API response body");
                }
                let body = resp.text().unwrap_or_default();
                let preview: String = body.chars().take(200).collect();
                if status.as_u16() == 429 || status.is_server_error() {
                    format!("HTTP {status}: {preview}")
                } else {
                    // 4xx other than 429 won't fix itself; fail this page now.
                    return Err(anyhow!("HTTP {status}: {preview}"));
                }
            }
            Err(err) => format!("network error: {err}"),
        };

        match backoffs.get(attempt) {
            Some(wait) => {
                eprintln!("warning: page={page_no} {retry_reason}; retrying in {wait:?}");
                thread::sleep(*wait);
                attempt += 1;
            }
            None => return Err(anyhow!("page={page_no} gave up after retries: {retry_reason}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/page_sa.json");

    /// Build a fake page body with `n` jobs whose ids start at `start`.
    fn fake_page(start: u32, n: u32, total: u64) -> String {
        let jobs: Vec<String> = (start..start + n)
            .map(|i| format!(r#"{{"data":{{"req_id":"{i}","slug":"{i}","title":"Job {i}"}}}}"#))
            .collect();
        format!(r#"{{"jobs":[{}],"totalCount":{total}}}"#, jobs.join(","))
    }

    #[test]
    fn parse_page_reads_fixture() {
        let page = parse_page(FIXTURE).unwrap();
        assert_eq!(page.jobs.len(), 4);
        assert_eq!(page.total_count, 4);
        let synthetic = page.jobs.iter().find(|j| j.req_id == "999049").unwrap();
        assert_eq!(synthetic.link(), "https://careers.heb.com/jobs/999049");
        assert!(synthetic.is_part_time());
    }

    #[test]
    fn parse_page_rejects_error_response() {
        let err = parse_page(r#"{"error":"Authentication required"}"#).unwrap_err();
        assert!(format!("{err:#}").contains("Authentication required"));
    }

    #[test]
    fn collect_stops_on_short_page() {
        let mut calls = 0;
        let jobs = collect_pages(
            |p| {
                calls += 1;
                Ok(if p == 1 { fake_page(0, 2, 99) } else { fake_page(10, 1, 99) })
            },
            2,
            10,
        )
        .unwrap();
        assert_eq!(calls, 2);
        assert_eq!(jobs.len(), 3);
    }

    #[test]
    fn collect_dedupes_req_id() {
        // Both pages contain job id "0".
        let jobs = collect_pages(
            |p| Ok(if p == 1 { fake_page(0, 1, 5) } else { fake_page(0, 1, 5) }),
            1,
            2,
        )
        .unwrap();
        assert_eq!(jobs.len(), 1);
    }

    #[test]
    fn collect_errors_when_all_pages_fail() {
        let err = collect_pages(|_| Err(anyhow!("HTTP 403")), 100, 3).unwrap_err();
        assert!(err.to_string().contains("every request"));
    }

    #[test]
    fn collect_errors_when_total_zero() {
        let err = collect_pages(|_| Ok(fake_page(0, 0, 0)), 100, 3).unwrap_err();
        assert!(err.to_string().contains("zero jobs"));
    }

    #[test]
    fn collect_tolerates_one_failed_page() {
        let jobs = collect_pages(
            |p| match p {
                1 => Ok(fake_page(0, 2, 10)),
                2 => Err(anyhow!("HTTP 500")),
                _ => Ok(fake_page(20, 1, 10)),
            },
            2,
            10,
        )
        .unwrap();
        let ids: Vec<&str> = jobs.iter().map(|j| j.req_id.as_str()).collect();
        assert_eq!(ids, ["0", "1", "20"]);
    }

    /// Hits the real API. Run with `cargo test -- --ignored live_fetch`.
    #[test]
    #[ignore]
    fn live_fetch_san_antonio() {
        let search = Search {
            location: "San Antonio, TX".into(),
            limit: 100,
            max_pages: 10,
        };
        let jobs = fetch_all(&search).unwrap();
        assert!(jobs.len() > 50, "only {} jobs", jobs.len());
        assert!(
            jobs.iter()
                .any(|j| j.location_name.as_deref().is_some_and(|l| l.starts_with("SA"))),
            "no store-coded jobs in San Antonio results"
        );
    }
}
