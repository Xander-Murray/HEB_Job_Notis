//! Deciding which jobs matter: is it at one of our stores, and what kind of
//! role is it?

use std::{cmp::Reverse, sync::OnceLock};

use regex::Regex;

use crate::{
    api::Job,
    config::{Config, Roles, Stores},
};

/// How a job was tied to one of our stores.
#[derive(Debug, Clone, PartialEq)]
pub enum StoreMatch {
    /// The store label carried one of our store numbers. Certain.
    Number(u32),
    /// No store number at all, but a street pair matched. Worth a look,
    /// but the email marks it "check location".
    Street,
    /// A store job posted for several locations, one of them in San Antonio.
    /// HEB doesn't say which stores, so it might include ours.
    MultiLocation,
}

/// Which email section a job belongs in.
///
/// Rust lesson: deriving `PartialOrd, Ord` on an enum orders the variants in
/// the order they're written, so `EntryLevel < Other`. That lets us sort jobs
/// with entry-level ones first for free.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    EntryLevel,
    Other,
}

/// A job that passed every filter, plus why.
#[derive(Debug, Clone)]
pub struct Hit {
    pub job: Job,
    pub store: StoreMatch,
    pub role: Role,
}

/// Pull a store number out of a store label, if it has one.
///
/// Only label-shaped text counts: the number must come right after a leading
/// `SA` (`SA49-Potranco`, `SA 50`, `SA049`) or a leading `San Antonio`
/// (`San Antonio 52 - ...`, `San Antonio 49 (Potranco/211) Curbie`). A number
/// anywhere else (`Pharmacy Tech 49 hrs`) is ignored.
pub fn store_number(text: &str) -> Option<u32> {
    // Rust lesson: `OnceLock` compiles each regex the first time it's needed
    // and reuses it afterwards. Compiling a regex is slow-ish, and this runs
    // for every job, so we don't want to redo it each call.
    //
    // Pattern notes: `(?i)` = ignore case, `^` = start of text, `\s*` =
    // optional spaces, `0*` = skip zero padding, `(\d{1,3})` = capture the
    // number, `\b` = the number must end there (so "SA490" reads as 490).
    static PATTERNS: OnceLock<[Regex; 2]> = OnceLock::new();
    let patterns = PATTERNS.get_or_init(|| {
        [
            Regex::new(r"(?i)^SA\s*0*(\d{1,3})\b").unwrap(),
            Regex::new(r"(?i)^San Antonio\s+0*(\d{1,3})\b").unwrap(),
        ]
    });

    let text = text.trim();
    // `find_map` tries each pattern and stops at the first one that yields
    // a number.
    patterns.iter().find_map(|re| {
        let caps = re.captures(text)?; // `?` on an Option: no match -> None
        caps[1].parse().ok()
    })
}

/// Is this job at one of our stores?
pub fn match_store(job: &Job, stores: &Stores) -> Option<StoreMatch> {
    // Look for a store number in the location label first, then the title.
    let number = job
        .location_name
        .as_deref()
        .and_then(store_number)
        .or_else(|| store_number(&job.title));

    if let Some(n) = number
        && stores.numbers.contains(&n)
    {
        return Some(StoreMatch::Number(n));
    }

    // A store job posted for several locations only names its primary store.
    // If San Antonio is among the others, one of them could be ours.
    if is_multi_location_store_job(job, stores) {
        return Some(StoreMatch::MultiLocation);
    }

    if number.is_some() {
        // The posting names a different store. Our store numbers are
        // confirmed, so that's not one of ours. No street backup.
        return None;
    }

    // Backup: no store number anywhere. Only consider San Antonio postings.
    let in_san_antonio = job
        .city
        .as_deref()
        .is_some_and(|c| c.eq_ignore_ascii_case("san antonio"));
    if !in_san_antonio {
        return None;
    }

    // Search everything that might name the street, as one lowercase string.
    let haystack = [job.location_name.as_deref(), Some(job.title.as_str()), job.street_address.as_deref()]
        .into_iter()
        .flatten() // drop the `None`s
        .collect::<Vec<_>>()
        .join(" ");

    // A pair matches only if *every* word in it appears as a whole word.
    let street_hit = stores
        .street_pairs
        .iter()
        .any(|pair| pair.iter().all(|word| has_word(&haystack, word, Boundary::Both)));
    street_hit.then_some(StoreMatch::Street)
}

/// Sort a job into a role, or `None` if it's a management role we skip.
pub fn classify(job: &Job, roles: &Roles) -> Option<Role> {
    // Keywords only need to *start* a word, so "bagger" also matches
    // "Baggers" and "estore" matches "eStores", while "csa" won't match "BCSA".
    let title_has = |word: &String| has_word(&job.title, word, Boundary::Start);

    if roles.exclude.iter().any(title_has) {
        return None;
    }
    if job.is_part_time() || roles.entry_level.iter().any(title_has) {
        Some(Role::EntryLevel)
    } else {
        Some(Role::Other)
    }
}

/// Run every job through store matching and role classification.
/// Results are sorted entry-level first, newest posting first within each.
pub fn select(jobs: &[Job], cfg: &Config) -> Vec<Hit> {
    let mut hits: Vec<Hit> = jobs
        .iter()
        .filter_map(|job| {
            // Inside `filter_map`, returning `None` drops the job. The `?`
            // operator returns `None` early if either check fails.
            let store = match_store(job, &cfg.stores)?;
            let role = classify(job, &cfg.roles)?;
            Some(Hit { job: job.clone(), store, role })
        })
        .collect();

    // Sort by role (entry-level first), then newest posted date first.
    // ISO-8601 dates sort correctly as plain strings. `Reverse` flips the order.
    hits.sort_by_key(|h| (h.role, Reverse(h.job.posted_date.clone())));
    hits
}

/// A multi-location posting in a store-level category that lists San Antonio.
fn is_multi_location_store_job(job: &Job, stores: &Stores) -> bool {
    let store_level = job
        .categories
        .iter()
        .any(|c| stores.multi_location_categories.iter().any(|want| want.eq_ignore_ascii_case(&c.name)));
    let lists_san_antonio = job
        .full_location
        .as_deref()
        .is_some_and(|l| l.to_lowercase().contains("san antonio"));
    job.multiple_locations && store_level && lists_san_antonio
}

/// Which word edges a keyword has to line up with.
#[derive(Clone, Copy)]
enum Boundary {
    /// Keyword must start a word (prefix match).
    Start,
    /// Keyword must be a whole word.
    Both,
}

/// Case-insensitive keyword search that respects word edges.
fn has_word(haystack: &str, word: &str, boundary: Boundary) -> bool {
    // `regex::escape` makes sure characters like `.` or `(` in a keyword are
    // matched literally instead of meaning something to the regex engine.
    let escaped = regex::escape(word);
    let pattern = match boundary {
        Boundary::Start => format!(r"(?i)\b{escaped}"),
        Boundary::Both => format!(r"(?i)\b{escaped}\b"),
    };
    // Keywords come from our own config file, so a bad pattern is a bug
    // worth crashing on loudly.
    Regex::new(&pattern).expect("keyword regex").is_match(haystack)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Test helper: a job with just the fields matching looks at.
    pub(crate) fn job(title: &str, location: Option<&str>) -> Job {
        Job {
            req_id: title.to_string(),
            slug: "1".into(),
            title: title.into(),
            location_name: location.map(String::from),
            city: Some("San Antonio".into()),
            street_address: None,
            posted_date: Some("2026-11-24T15:00:00+0000".into()),
            employment_type: Some("FULL_TIME".into()),
            tags1: None,
            multiple_locations: false,
            full_location: None,
            categories: vec![],
        }
    }

    /// Test helper: a multi-location posting.
    fn multi(location: &str, full_location: &str, category: &str) -> Job {
        let mut j = job("Front End - Customer Service Assistant", Some(location));
        j.city = Some("Spring Branch".into());
        j.multiple_locations = true;
        j.full_location = Some(full_location.into());
        j.categories = vec![crate::api::Category { name: category.into() }];
        j
    }

    fn stores() -> Stores {
        Stores {
            numbers: vec![49, 50, 52],
            street_pairs: vec![
                vec!["potranco".into(), "211".into()],
                vec!["culebra".into(), "211".into()],
                vec!["alamo ranch".into()],
            ],
            multi_location_categories: vec!["Store Operations".into(), "Pharmacy".into()],
        }
    }

    #[test]
    fn match_store_multi_location_store_job() {
        let sa = "Spring Branch, Texas; San Antonio, Texas; San Antonio, Texas";
        let j = multi("Bulverde-Hwy 46 West", sa, "Store Operations");
        assert_eq!(match_store(&j, &stores()), Some(StoreMatch::MultiLocation));
        // Primary store is a different one, but the others might be ours.
        let j = multi("SA11-Valley Hi", sa, "Pharmacy");
        assert_eq!(match_store(&j, &stores()), Some(StoreMatch::MultiLocation));
        // Primary store is ours: that's a certain match.
        let j = multi("SA49-Potranco", sa, "Store Operations");
        assert_eq!(match_store(&j, &stores()), Some(StoreMatch::Number(49)));
    }

    #[test]
    fn match_store_multi_location_ignores_non_store_or_non_sa() {
        let sa = "Austin, Texas; San Antonio, Texas";
        assert_eq!(match_store(&multi("Headquarters-Arsenal", sa, "eCommerce"), &stores()), None);
        let j = multi("Bulverde-Hwy 46 West", "Austin, Texas; Dallas, Texas", "Store Operations");
        assert_eq!(match_store(&j, &stores()), None);
    }

    fn roles() -> Roles {
        Roles {
            exclude: ["manager", "director", "supervisor", "unit director", "assistant director"]
                .map(String::from)
                .to_vec(),
            entry_level: [
                "curbie", "bagger", "checker", "cashier", "customer service", "service rep", "csa",
                "courtesy", "store partner", "stocker", "estore",
            ]
            .map(String::from)
            .to_vec(),
        }
    }

    #[test]
    fn store_number_reads_label_forms() {
        assert_eq!(store_number("SA49-Potranco"), Some(49));
        assert_eq!(store_number("SA 50 Culebra"), Some(50));
        assert_eq!(store_number("SA049-x"), Some(49));
        assert_eq!(store_number("San Antonio 52 - Alamo Ranch"), Some(52));
        assert_eq!(store_number("San Antonio 49 (Potranco/211) Curbie - Part time"), Some(49));
        assert_eq!(store_number("SA02 CM-Broadway/Patterson"), Some(2));
    }

    #[test]
    fn store_number_ignores_non_labels() {
        assert_ne!(store_number("SA490"), Some(49));
        assert_ne!(store_number("SA5"), Some(49));
        assert_ne!(store_number("SA 4"), Some(49));
        assert_eq!(store_number("Pharmacy Tech 49 hrs"), None);
        assert_eq!(store_number("Headquarters-Arsenal"), None);
        assert_eq!(store_number("Senior Analyst San Antonio 50"), None);
        assert_eq!(store_number("SA Super Regional Whse"), None);
        assert_eq!(store_number("San Antonio Retail Support Center (SARSC)"), None);
    }

    #[test]
    fn match_store_by_number() {
        let j = job("Curbie", Some("SA49-Potranco/SH 211"));
        assert_eq!(match_store(&j, &stores()), Some(StoreMatch::Number(49)));
        let j = job("San Antonio 52 (Alamo Ranch) Checker", Some("Something"));
        assert_eq!(match_store(&j, &stores()), Some(StoreMatch::Number(52)));
    }

    #[test]
    fn match_store_other_number_never_matches_by_street() {
        let j = job("Curbie", Some("SA11-Culebra/211"));
        assert_eq!(match_store(&j, &stores()), None);
    }

    #[test]
    fn match_store_street_backup() {
        let j = job("Curbie", Some("Culebra & SH 211"));
        assert_eq!(match_store(&j, &stores()), Some(StoreMatch::Street));
        let j = job("Bagger", Some("Alamo Ranch Pkwy"));
        assert_eq!(match_store(&j, &stores()), Some(StoreMatch::Street));
    }

    #[test]
    fn match_store_street_pair_must_be_complete() {
        let j = job("Curbie", Some("Culebra/Tezel"));
        assert_eq!(match_store(&j, &stores()), None);
        // "2110" is not "211".
        let j = job("Curbie", Some("Culebra/2110 Main"));
        assert_eq!(match_store(&j, &stores()), None);
    }

    #[test]
    fn match_store_street_requires_san_antonio() {
        let mut j = job("Curbie", Some("Alamo Ranch Pkwy"));
        j.city = Some("Austin".into());
        assert_eq!(match_store(&j, &stores()), None);
    }

    #[test]
    fn classify_excludes_management() {
        assert_eq!(classify(&job("Store Manager", None), &roles()), None);
        assert_eq!(classify(&job("Assistant Store Director", None), &roles()), None);
    }

    #[test]
    fn classify_entry_level_by_keyword_or_part_time() {
        assert_eq!(classify(&job("Curbie - Part time", None), &roles()), Some(Role::EntryLevel));
        assert_eq!(classify(&job("Courtesy Clerks", None), &roles()), Some(Role::EntryLevel));
        assert_eq!(classify(&job("Prosper eStores - Shopper", None), &roles()), Some(Role::EntryLevel));
        let mut rx = job("Pharmacy Tech", None);
        rx.employment_type = Some("PART_TIME".into());
        assert_eq!(classify(&rx, &roles()), Some(Role::EntryLevel));
    }

    #[test]
    fn classify_other_roles() {
        assert_eq!(classify(&job("Pharmacy Technician", None), &roles()), Some(Role::Other));
        // "csa" must start a word: "BCSA" is not a CSA role.
        assert_eq!(classify(&job("BCSA Coordinator", None), &roles()), Some(Role::Other));
    }

    #[test]
    fn select_filters_and_sorts() {
        let mut old_entry = job("Checker", Some("SA50-Culebra/211"));
        old_entry.posted_date = Some("2026-11-01T00:00:00+0000".into());
        let new_entry = job("Bagger", Some("SA52-Alamo Ranch"));
        let other = job("Pharmacy Technician", Some("SA49-Potranco"));
        let manager = job("Store Manager", Some("SA49-Potranco"));
        let elsewhere = job("Curbie", Some("SA11-Valley Hi"));

        let cfg = Config {
            search: crate::config::Search { location: String::new(), limit: 100, max_pages: 1 },
            stores: stores(),
            roles: roles(),
        };
        let hits = select(&[other, old_entry, manager, elsewhere, new_entry], &cfg);
        let titles: Vec<&str> = hits.iter().map(|h| h.job.title.as_str()).collect();
        assert_eq!(titles, ["Bagger", "Checker", "Pharmacy Technician"]);
    }
}
