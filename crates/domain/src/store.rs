//! Durable, restartable state — the persistence seam.
//!
//! This defines the `StateStore` contract plus a std-only file-backed default,
//! enough to prove durability and restartable load. The
//! SQLite + sqlite-vec backend is provided by the memory store in a later phase.
//!
//! `StateStore` moved to `cronus-contract`; the
//! implementation (`FileStore`) stays here, in the domain tier.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::PathBuf;

pub use cronus_contract::StateStore;

/// File-backed store (foundation). Persists `key\tvalue` lines and loads them on
/// open. Keys and values must not contain a tab or newline (provisional format;
/// the SQLite backend supersedes it).
#[derive(Debug)]
pub struct FileStore {
    path: PathBuf,
    data: BTreeMap<String, String>,
}

impl FileStore {
    /// Open (or create) a store at `path`, loading any existing entries.
    pub fn open(path: impl Into<PathBuf>) -> io::Result<Self> {
        let path = path.into();
        let mut data = BTreeMap::new();
        if path.exists() {
            for line in fs::read_to_string(&path)?.lines() {
                if let Some((key, value)) = line.split_once('\t') {
                    data.insert(key.to_string(), value.to_string());
                }
            }
        } else if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(Self { path, data })
    }

    fn flush(&self) -> io::Result<()> {
        let mut out = String::new();
        for (key, value) in &self.data {
            out.push_str(key);
            out.push('\t');
            out.push_str(value);
            out.push('\n');
        }
        fs::write(&self.path, out)
    }
}

impl StateStore for FileStore {
    fn put(&mut self, key: &str, value: &str) -> io::Result<()> {
        self.data.insert(key.to_string(), value.to_string());
        self.flush()
    }

    fn get(&self, key: &str) -> Option<String> {
        self.data.get(key).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("cronus-store-{tag}-{}.kv", std::process::id()))
    }

    #[test]
    fn survives_a_restart() {
        let path = temp_path("restart");
        let _ = fs::remove_file(&path);

        {
            let mut store = FileStore::open(&path).expect("open");
            store.put("phase", "1").expect("put");
            store.put("seed", "planted").expect("put");
        } // dropped — simulates process exit

        let reopened = FileStore::open(&path).expect("reopen");
        assert_eq!(reopened.get("phase").as_deref(), Some("1"));
        assert_eq!(reopened.get("seed").as_deref(), Some("planted"));
        assert_eq!(reopened.get("missing"), None);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn open_missing_path_starts_empty() {
        let path = temp_path("empty");
        let _ = fs::remove_file(&path);
        let store = FileStore::open(&path).expect("open");
        assert_eq!(store.get("anything"), None);
    }
}
