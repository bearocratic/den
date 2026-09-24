//! The folders the menu bar watches.
//!
//! One list, in one file, read and written by both surfaces: the app
//! shows what is in it, and `den <folder>` in a terminal puts what it
//! was pointed at into it. The CLI's own sessions are keyed by the
//! exact set of folders they were opened with, which makes them a
//! record of what you did rather than a list of what you watch.

use crate::session;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// How deep into a folder a repository may sit, when nothing says otherwise.
pub const DEFAULT_DEPTH: usize = 4;
/// A menu bar app runs all day, so it fetches far less often than the TUI.
pub const DEFAULT_FETCH_SECS: u64 = 900;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BarConfig {
    #[serde(default)]
    pub folders: Vec<PathBuf>,
    #[serde(default)]
    pub depth: Option<usize>,
    #[serde(default)]
    pub fetch_interval_secs: Option<u64>,
}

impl BarConfig {
    pub fn path() -> PathBuf {
        session::den_dir().join("bar.toml")
    }

    pub fn load() -> Self {
        if let Ok(text) = std::fs::read_to_string(Self::path()) {
            if let Ok(cfg) = toml::from_str::<BarConfig>(&text) {
                return cfg;
            }
        }
        let mut cfg = BarConfig::default();
        for (_, bases, _) in session::list_sessions() {
            for base in bases {
                if !cfg.folders.contains(&base) {
                    cfg.folders.push(base);
                }
            }
        }
        cfg.folders = prune_nested(cfg.folders);
        cfg.save();
        cfg
    }

    /// Add a folder unless it is already watched, or already inside
    /// something watched. Returns whether the list changed.
    pub fn add_folder(&mut self, folder: PathBuf) -> bool {
        if self.folders.iter().any(|f| folder.starts_with(f)) {
            return false;
        }
        self.folders.push(folder);
        self.folders = prune_nested(std::mem::take(&mut self.folders));
        true
    }

    pub fn save(&self) {
        session::ensure_dir(&session::den_dir());
        if let Ok(text) = toml::to_string_pretty(self) {
            let _ = std::fs::write(Self::path(), text);
        }
    }

    /// Take in folders someone opened in a terminal.
    ///
    /// Returns whether anything changed, so a caller can stay quiet
    /// when there is nothing to say.
    pub fn adopt(folders: &[PathBuf]) -> bool {
        let mut cfg = BarConfig::load();
        let mut changed = false;
        for folder in folders {
            let folder = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.clone());
            if cfg.add_folder(folder) {
                changed = true;
            }
        }
        if changed {
            cfg.save();
        }
        changed
    }

    pub fn depth(&self) -> usize {
        self.depth.unwrap_or(DEFAULT_DEPTH)
    }

    pub fn fetch_secs(&self) -> u64 {
        self.fetch_interval_secs.unwrap_or(DEFAULT_FETCH_SECS)
    }
}

/// Watching a folder and a folder inside it lists every nested
/// repository twice, so the inner one goes.
fn prune_nested(folders: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut kept: Vec<PathBuf> = Vec::new();
    for folder in folders {
        if kept.iter().any(|k| folder.starts_with(k)) {
            continue;
        }
        kept.retain(|k| !k.starts_with(&folder));
        kept.push(folder);
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::prune_nested;
    use std::path::PathBuf;

    /// How deep into a folder a repository may sit, when nothing says otherwise.
    pub const DEFAULT_DEPTH: usize = 4;
    /// A menu bar app runs all day, so it fetches far less often than the TUI.
    pub const DEFAULT_FETCH_SECS: u64 = 900;

    fn paths(v: &[&str]) -> Vec<PathBuf> {
        v.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn a_folder_inside_another_watched_folder_is_dropped() {
        assert_eq!(
            prune_nested(paths(&["/work", "/work/den", "/personal"])),
            paths(&["/work", "/personal"])
        );
    }

    #[test]
    fn the_outer_folder_wins_even_when_it_arrives_second() {
        assert_eq!(
            prune_nested(paths(&["/work/den", "/work"])),
            paths(&["/work"])
        );
    }

    #[test]
    fn a_shared_prefix_is_not_containment() {
        assert_eq!(
            prune_nested(paths(&["/work", "/work-notes"])),
            paths(&["/work", "/work-notes"])
        );
    }
}
