//! Secret access: read from the environment first, then the
//! state-tier `.env`. Secret values are never logged; callers must pass values
//! through `crate::redact` before rendering.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// Read a secret by key. The process environment wins; otherwise the `.env`
/// file at `env_file` (typically `<state>/.env`) is consulted. Returns `None`
/// if unset. The value is never written to any log by this function.
pub fn get(key: &str, env_file: Option<&Path>) -> Option<String> {
    if let Ok(value) = std::env::var(key)
        && !value.is_empty()
    {
        return Some(value);
    }
    let text = fs::read_to_string(env_file?).ok()?;
    parse_env(&text).remove(key)
}

/// Parse a minimal `.env` (KEY=VALUE per line; `#` comments; optional quotes).
fn parse_env(text: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let value = value.trim().trim_matches('"');
            map.insert(key.trim().to_string(), value.to_string());
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_var_takes_precedence() {
        // SAFETY: single-threaded test; no other thread reads this var.
        unsafe { std::env::set_var("CRONUS_TEST_SECRET", "from-env") };
        assert_eq!(get("CRONUS_TEST_SECRET", None).as_deref(), Some("from-env"));
        unsafe { std::env::remove_var("CRONUS_TEST_SECRET") };
    }

    #[test]
    fn reads_from_env_file_when_unset() {
        let path = std::env::temp_dir().join(format!("cronus-env-{}.env", std::process::id()));
        fs::write(&path, "# comment\nKEY_A = \"value-a\"\nKEY_B=plain\n").unwrap();

        assert_eq!(get("KEY_A", Some(&path)).as_deref(), Some("value-a"));
        assert_eq!(get("KEY_B", Some(&path)).as_deref(), Some("plain"));
        assert_eq!(get("ABSENT", Some(&path)), None);

        let _ = fs::remove_file(&path);
    }
}
