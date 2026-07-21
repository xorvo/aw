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

/// The `agent_config:` section — reserved (not a base) and previously
/// opaque. `resume_commands` maps an agent name to the shell command
/// `aw resurrect` types into a restored pane to pick the conversation back
/// up. Unknown extra keys are tolerated.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct AgentConfig {
    #[serde(default)]
    pub resume_commands: BTreeMap<String, String>,
}

/// Built-in resume commands, overridable per agent via
/// `agent_config.resume_commands`. Two tiers:
///
/// - When the hook payload gave us the conversation id, resume *that exact
///   conversation* — precise even with several sessions in one directory.
/// - Otherwise fall back to the `--continue`-style form, which reopens the
///   directory's most recent conversation non-interactively (no session
///   picker, since resurrect runs with nobody at the keyboard).
const DEFAULT_RESUME_WITH_ID: &[(&str, &str)] = &[
    ("claude", "claude --resume {session_id}"),
    ("codex", "codex resume {session_id}"),
    ("opencode", "opencode --session {session_id}"),
    ("kimi", "kimi --session {session_id}"),
];
const DEFAULT_RESUME_COMMANDS: &[(&str, &str)] = &[
    ("claude", "claude --continue"),
    ("codex", "codex resume --last"),
    ("opencode", "opencode --continue"),
    ("kimi", "kimi --continue"),
];

#[derive(Debug, Clone)]
pub struct Config {
    /// Insertion-stable mapping of base name -> Base. We use BTreeMap for
    /// deterministic ordering when listing bases (the bash version sorts by
    /// `keys | .[]` which yq emits in document order; alphabetical via
    /// BTreeMap is close enough and stable across runs.)
    pub bases: BTreeMap<String, Base>,
    pub agent_config: AgentConfig,
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
        let mut agent_config = AgentConfig::default();
        for (k, v) in mapping {
            let key = match k {
                serde_yaml::Value::String(s) => s,
                _ => continue, // ignore non-string keys
            };
            if key == "agent_config" {
                // Tolerant like the bases: a malformed section is ignored
                // rather than failing the whole config.
                agent_config = serde_yaml::from_value(v).unwrap_or_default();
                continue;
            }
            if RESERVED_KEYS.contains(&key.as_str()) {
                continue;
            }
            // Tolerate `repos:` written with a trailing comment and no list
            // (parses to Null) — treat it as empty.
            let base: Base = serde_yaml::from_value(v).unwrap_or_default();
            bases.insert(key, base);
        }
        Ok(Self { bases, agent_config })
    }

    /// Load the config if present; a missing file yields an empty config so
    /// callers that only need defaults (e.g. resume commands) still work.
    pub fn load_or_default(path: &Path) -> Self {
        if path.is_file() {
            Self::load(path).unwrap_or_else(|_| Self {
                bases: BTreeMap::new(),
                agent_config: AgentConfig::default(),
            })
        } else {
            Self {
                bases: BTreeMap::new(),
                agent_config: AgentConfig::default(),
            }
        }
    }

    /// Resume command for an agent, preferring the exact conversation when
    /// its id is known. Resolution order:
    ///
    /// 1. User override (`agent_config.resume_commands`). May contain a
    ///    `{session_id}` placeholder; it's substituted (shell-quoted) when
    ///    an id is available, otherwise the override is skipped and the
    ///    defaults apply. An empty-string override disables resumption
    ///    (plain shell).
    /// 2. Built-in id template when an id is available.
    /// 3. Built-in `--continue`-style fallback.
    pub fn resume_command(&self, agent: &str, session_id: Option<&str>) -> Option<String> {
        let sid = session_id.map(str::trim).filter(|s| !s.is_empty());
        if let Some(cmd) = self.agent_config.resume_commands.get(agent) {
            let cmd = cmd.trim();
            if cmd.is_empty() {
                return None;
            }
            match (cmd.contains("{session_id}"), sid) {
                (true, Some(id)) => {
                    return Some(cmd.replace(
                        "{session_id}",
                        &crate::workspace::start::sh_quote(id),
                    ));
                }
                (true, None) => {} // placeholder unusable — fall through to defaults
                (false, _) => return Some(cmd.to_string()),
            }
        }
        if let Some(id) = sid {
            if let Some((_, t)) = DEFAULT_RESUME_WITH_ID.iter().find(|(a, _)| *a == agent) {
                return Some(t.replace(
                    "{session_id}",
                    &crate::workspace::start::sh_quote(id),
                ));
            }
        }
        DEFAULT_RESUME_COMMANDS
            .iter()
            .find(|(a, _)| *a == agent)
            .map(|(_, c)| c.to_string())
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

    #[test]
    fn resume_command_defaults_without_id() {
        let c = Config::parse("").unwrap();
        assert_eq!(c.resume_command("claude", None).as_deref(), Some("claude --continue"));
        assert_eq!(c.resume_command("codex", None).as_deref(), Some("codex resume --last"));
        assert_eq!(c.resume_command("opencode", None).as_deref(), Some("opencode --continue"));
        assert_eq!(c.resume_command("kimi", None).as_deref(), Some("kimi --continue"));
        assert_eq!(c.resume_command("pi", None), None);
    }

    #[test]
    fn resume_command_prefers_exact_session_id() {
        let c = Config::parse("").unwrap();
        assert_eq!(
            c.resume_command("claude", Some("abc-123")).as_deref(),
            Some("claude --resume 'abc-123'")
        );
        assert_eq!(
            c.resume_command("codex", Some("r1")).as_deref(),
            Some("codex resume 'r1'")
        );
        // Blank id degrades to the --continue default.
        assert_eq!(c.resume_command("claude", Some("  ")).as_deref(), Some("claude --continue"));
        // Unknown agent stays None even with an id.
        assert_eq!(c.resume_command("pi", Some("x")), None);
    }

    #[test]
    fn resume_command_overrides_and_disables() {
        let yaml = "
agent_config:
  resume_commands:
    claude: 'claude --resume {session_id} --verbose'
    codex: ''
    pi: pi --restore
";
        let c = Config::parse(yaml).unwrap();
        assert_eq!(
            c.resume_command("claude", Some("abc")).as_deref(),
            Some("claude --resume 'abc' --verbose")
        );
        // Placeholder override without an id falls back to the default.
        assert_eq!(c.resume_command("claude", None).as_deref(), Some("claude --continue"));
        assert_eq!(c.resume_command("codex", Some("x")), None, "empty override disables");
        // Plain override ignores the id — the user asked for this exact command.
        assert_eq!(c.resume_command("pi", Some("x")).as_deref(), Some("pi --restore"));
        // Untouched agents keep their defaults.
        assert_eq!(c.resume_command("kimi", None).as_deref(), Some("kimi --continue"));
    }

    #[test]
    fn malformed_agent_config_is_tolerated() {
        let c = Config::parse("agent_config: just a string\ndefault:\n  repos: []\n").unwrap();
        assert!(c.agent_config.resume_commands.is_empty());
        assert_eq!(c.base_names(), vec!["default"]);
    }
}
