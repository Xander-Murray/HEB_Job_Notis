//! Loading `config.toml` into typed Rust structs.
//!
//! Rust lesson: `#[derive(Deserialize)]` asks the `serde` crate to generate
//! code at compile time that turns TOML (or JSON) into these structs. If the
//! file is missing a field or has the wrong type, loading fails with an error
//! instead of silently using a default -- the opposite of Python's
//! `dict.get("x", "")`.

use std::{fs, path::Path};

use anyhow::{Context, Result};
use serde::Deserialize;

/// The whole config file. Each field maps to a `[section]` in the TOML.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub search: Search,
    pub stores: Stores,
    pub roles: Roles,
}

/// How to query HEB's API.
#[derive(Debug, Clone, Deserialize)]
pub struct Search {
    pub location: String,
    pub limit: u32,
    pub max_pages: u32,
}

/// Which stores count as a match.
#[derive(Debug, Clone, Deserialize)]
pub struct Stores {
    pub numbers: Vec<u32>,
    pub street_pairs: Vec<Vec<String>>,
}

/// Keyword lists for classifying a job's role.
#[derive(Debug, Clone, Deserialize)]
pub struct Roles {
    pub exclude: Vec<String>,
    pub entry_level: Vec<String>,
}

impl Config {
    /// Read and parse a config file.
    ///
    /// Rust lesson: the return type `Result<Config>` means "either a Config or
    /// an error". The `?` operator after a call says "if this failed, return
    /// the error from *this* function right now". `.with_context(...)` wraps
    /// the error with a human-readable note so you know which file broke.
    pub fn load(path: &Path) -> Result<Config> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("reading config file {}", path.display()))?;
        let config: Config = toml::from_str(&text)
            .with_context(|| format!("parsing config file {}", path.display()))?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_repo_config() {
        // CARGO_MANIFEST_DIR is the folder holding Cargo.toml, so this test
        // checks the real config.toml that CI will use.
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("config.toml");
        let cfg = Config::load(&path).expect("config.toml should load");
        assert_eq!(cfg.stores.numbers, vec![49, 50, 52]);
        assert_eq!(cfg.stores.street_pairs.len(), 3);
        assert!(cfg.roles.exclude.contains(&"manager".to_string()));
    }
}
