//! Canned config templates for tests.

use super::TestEnv;

/// A config with one base "default" referencing a single fake remote.
pub fn config_with_one_remote(env: &TestEnv, repo: &str) -> String {
    format!(
        "default:\n  repos:\n    - {}\n  local_files: []\n",
        env.remote_url(repo)
    )
}

/// A config with one base "default" referencing two fake remotes.
pub fn config_with_two_remotes(env: &TestEnv, a: &str, b: &str) -> String {
    format!(
        "default:\n  repos:\n    - {}\n    - {}\n  local_files: []\n",
        env.remote_url(a),
        env.remote_url(b),
    )
}

/// A config with one remote and one local file (with optional rename).
pub fn config_with_remote_and_local(
    env: &TestEnv,
    repo: &str,
    local: &str,
    rename_to: Option<&str>,
) -> String {
    let local_entry = match rename_to {
        Some(name) => format!("{} -> {}", env.local_path(local), name),
        None => env.local_path(local),
    };
    format!(
        "default:\n  repos:\n    - {}\n  local_files:\n    - \"{}\"\n",
        env.remote_url(repo),
        local_entry,
    )
}

/// Two named bases: `default` with one remote, `dev` with a different remote.
pub fn config_with_two_bases(env: &TestEnv, default_repo: &str, dev_repo: &str) -> String {
    format!(
        "default:\n  repos:\n    - {}\n  local_files: []\n\
         dev:\n  repos:\n    - {}\n  local_files: []\n",
        env.remote_url(default_repo),
        env.remote_url(dev_repo),
    )
}
