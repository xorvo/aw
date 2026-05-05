//! Config file (`config.yaml`) parser.
//!
//! Schema mirrors the bash CLI's expectations: top-level keys are base names,
//! each with optional `repos: [...]` and `local_files: [...]` lists. The keys
//! `agent_config` and `workspace_defaults` are reserved (not bases) and
//! filtered when listing available bases.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

/// Base names treated as configuration sections, not workspace templates.
/// Mirrors the `grep -v` filters in the bash `show_config`.
const RESERVED_KEYS: &[&str] = &["agent_config", "workspace_defaults"];

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Base {
    #[serde(default)]
    pub repos: Vec<String>,
    #[serde(default)]
    pub local_files: Vec<String>,
    /// Free-form, displayed only in the (yet-to-be-built) `aw config show
    /// <base>` detail view. Tolerated in YAML so user comments/descriptions
    /// don't fail to parse.
    #[serde(default)]
    #[allow(dead_code)]
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Config {
    /// Insertion-stable mapping of base name -> Base. We use BTreeMap for
    /// deterministic ordering when listing bases (the bash version sorts by
    /// `keys | .[]` which yq emits in document order; alphabetical via
    /// BTreeMap is close enough and stable across runs.)
    pub bases: BTreeMap<String, Base>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("read config: {}", path.display()))?;
        Self::parse(&raw)
    }

    /// Parse YAML into Config. Reserved keys (agent_config, workspace_defaults)
    /// are silently dropped — they're not bases.
    pub fn parse(yaml: &str) -> Result<Self> {
        // We accept a Mapping at the top level; each value is either a Base
        // (repos/local_files) or arbitrary config. To stay tolerant of the
        // reserved keys' shapes, we pre-filter them out and then deserialize
        // the remainder.
        let value: serde_yaml::Value = serde_yaml::from_str(yaml)
            .context("invalid YAML in config")?;
        let mapping = match value {
            serde_yaml::Value::Mapping(m) => m,
            serde_yaml::Value::Null => serde_yaml::Mapping::new(),
            _ => return Err(anyhow!("config root must be a mapping")),
        };

        let mut bases = BTreeMap::new();
        for (k, v) in mapping {
            let key = match k {
                serde_yaml::Value::String(s) => s,
                _ => continue, // ignore non-string keys
            };
            if RESERVED_KEYS.contains(&key.as_str()) {
                continue;
            }
            // Tolerate `repos:` written with a trailing comment and no list
            // (parses to Null) — treat it as empty.
            let base: Base = serde_yaml::from_value(v).unwrap_or_default();
            bases.insert(key, base);
        }
        Ok(Self { bases })
    }

    /// Look up a base by name.
    pub fn base(&self, name: &str) -> Option<&Base> {
        self.bases.get(name)
    }

    /// Names of all configured bases, sorted (deterministic).
    pub fn base_names(&self) -> Vec<&str> {
        self.bases.keys().map(String::as_str).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal() {
        let c = Config::parse("default:\n  repos: []\n  local_files: []\n").unwrap();
        assert_eq!(c.base_names(), vec!["default"]);
        assert!(c.base("default").unwrap().repos.is_empty());
    }

    #[test]
    fn drops_reserved_keys() {
        let yaml = "
default:
  repos: []
agent_config:
  foo: bar
workspace_defaults:
  any: thing
dev:
  repos: []
";
        let c = Config::parse(yaml).unwrap();
        assert_eq!(c.base_names(), vec!["default", "dev"]);
    }

    #[test]
    fn null_repos_is_empty() {
        let yaml = "default:\n  repos:\n  local_files:\n";
        let c = Config::parse(yaml).unwrap();
        assert!(c.base("default").unwrap().repos.is_empty());
        assert!(c.base("default").unwrap().local_files.is_empty());
    }

    #[test]
    fn rejects_non_mapping_root() {
        assert!(Config::parse("- not a map\n").is_err());
    }

    #[test]
    fn null_root_is_empty() {
        let c = Config::parse("").unwrap();
        assert!(c.bases.is_empty());
    }
}
