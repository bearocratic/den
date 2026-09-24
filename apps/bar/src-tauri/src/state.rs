//! What the panel is shown, and where the bar keeps its folders.
//!
//! The scan itself is `den_core`; this module only shapes the result
//! for a webview and remembers which folders the menu bar watches.

use den_core::bar::BarConfig;
use den_core::repo::{discover, status_for, RepoStatus};
use den_core::{display_order, session, CiInfo, CiState, OrderView, SortMode};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// A name for each folder, long enough to tell them apart.
///
/// The last segment is usually enough — `~/work` is "work". When two
/// folders end in the same word, both grow a segment to the left, and
/// keep growing until they differ, so `~/work/projects` and
/// `~/personal/projects` never both read "projects".
pub fn label_folders(folders: &[PathBuf]) -> Vec<String> {
    let parts: Vec<Vec<String>> = folders
        .iter()
        .map(|f| {
            f.components()
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .filter(|s| s != "/")
                .collect()
        })
        .collect();

    let mut labels: Vec<String> = Vec::with_capacity(folders.len());
    for (i, mine) in parts.iter().enumerate() {
        let mut depth = 1;
        while depth < mine.len() {
            let tail = |p: &Vec<String>| p[p.len().saturating_sub(depth)..].join("/");
            let me = tail(mine);
            let clashes = parts
                .iter()
                .enumerate()
                .any(|(j, other)| j != i && tail(other) == me);
            if !clashes {
                break;
            }
            depth += 1;
        }
        let label = mine[mine.len().saturating_sub(depth)..].join("/");
        labels.push(if label.is_empty() {
            folders[i].display().to_string()
        } else {
            label
        });
    }
    labels
}

// ── what the panel receives ───────────────────────────────────────

/// The one thing the menu bar glyph says, worst state first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Badge {
    Failing,
    Dirty,
    Clean,
}

#[derive(Debug, Clone, Serialize)]
pub struct CiView {
    pub state: CiState,
    pub name: String,
    pub url: String,
    pub failed_step: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepoView {
    pub name: String,
    pub path: String,
    pub branch: Option<String>,
    pub ahead: usize,
    pub behind: usize,
    pub staged: usize,
    pub modified: usize,
    pub untracked: usize,
    pub conflicted: usize,
    pub stashed: usize,
    pub tag: Option<String>,
    pub commits_since_tag: usize,
    pub last_commit: Option<String>,
    pub error: Option<String>,
    pub uninitialized: bool,
    pub pinned: bool,
    pub ci: Option<CiView>,
    /// "conflict" | "error" | "dirty" | "clean" | "empty"
    pub state: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct FolderView {
    pub path: String,
    pub label: String,
    pub total: usize,
    pub dirty: usize,
    pub failing: usize,
    pub missing: bool,
    pub repos: Vec<RepoView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub folders: Vec<FolderView>,
    /// The version running right now, so the menu can say it.
    pub version: String,
    /// Whether this build can fetch its own next version at all.
    pub updatable: bool,
    pub update: Option<String>,
    pub checking: bool,
    /// Seconds since the last answer, if it is recent enough to show.
    pub checked_secs: Option<u64>,
    pub badge: Badge,
    pub scanning: bool,
    pub ci_age_secs: Option<u64>,
    pub gh: bool,
}

/// Everything the bar holds between scans.
pub struct Model {
    pub version: String,
    pub updatable: bool,
    pub cfg: BarConfig,
    pub repos: Vec<RepoStatus>,
    pub ci: HashMap<PathBuf, CiInfo>,
    pub ci_at: Option<SystemTime>,
    pub gh: bool,
    pub scanning: bool,
    /// The version waiting to be installed, once a check has found one.
    pub update: Option<String>,
    /// A check is running because someone asked for it.
    pub checking: bool,
    /// When someone last asked, so "up to date" can stop being said.
    pub checked_at: Option<SystemTime>,
    pub hidden: HashSet<PathBuf>,
    pub pinned: HashSet<PathBuf>,
}

impl Model {
    pub fn new(version: String, updatable: bool) -> Self {
        let cfg = BarConfig::load();
        let mut m = Model {
            version,
            updatable,
            cfg,
            repos: Vec::new(),
            ci: HashMap::new(),
            ci_at: None,
            gh: false,
            scanning: false,
            update: None,
            checking: false,
            checked_at: None,
            hidden: HashSet::new(),
            pinned: HashSet::new(),
        };
        m.reload_marks();
        m
    }

    /// Pins live with the session for this exact folder set, the way
    /// the TUI writes them; hiding is global to den.
    pub fn reload_marks(&mut self) {
        let session_id = session::resolve_session(&self.cfg.folders);
        self.pinned = session::load_path_set(&session::session_dir(&session_id).join("pins.txt"));
        self.hidden = session::load_path_set(&session::den_dir().join("hidden.txt"));
    }

    /// Scan without a lock held: the panel keeps answering while it runs.
    pub fn scan_folders(folders: &[PathBuf], depth: usize) -> Vec<RepoStatus> {
        let mut repos = Vec::new();
        for folder in folders {
            for path in discover(folder, depth) {
                repos.push(status_for(&path));
            }
        }
        repos
    }

    pub fn snapshot(&self) -> Snapshot {
        let labels = label_folders(&self.cfg.folders);
        let folders: Vec<FolderView> = self
            .cfg
            .folders
            .iter()
            .zip(labels)
            .map(|(base, label)| self.folder_view(base, label))
            .collect();

        let badge = if folders.iter().any(|f| f.failing > 0) {
            Badge::Failing
        } else if folders.iter().any(|f| f.dirty > 0) {
            Badge::Dirty
        } else {
            Badge::Clean
        };

        Snapshot {
            folders,
            version: self.version.clone(),
            updatable: self.updatable,
            update: self.update.clone(),
            checking: self.checking,
            checked_secs: self
                .checked_at
                .and_then(|t| SystemTime::now().duration_since(t).ok())
                .map(|d| d.as_secs()),
            badge,
            scanning: self.scanning,
            ci_age_secs: self
                .ci_at
                .and_then(|t| SystemTime::now().duration_since(t).ok())
                .map(|d| d.as_secs()),
            gh: self.gh,
        }
    }

    fn folder_view(&self, base: &Path, label: String) -> FolderView {
        let mine: Vec<RepoStatus> = self
            .repos
            .iter()
            .filter(|r| r.path.starts_with(base))
            .cloned()
            .collect();

        let order = display_order(&OrderView {
            repos: &mine,
            filter_query: "",
            show_hidden: false,
            hidden: &self.hidden,
            pinned: &self.pinned,
            bases: &[],
            base_filter: None,
            sort_mode: SortMode::Default,
        });

        let repos: Vec<RepoView> = order.iter().map(|i| self.repo_view(&mine[*i])).collect();
        let dirty = repos
            .iter()
            .filter(|r| r.state == "dirty" || r.state == "conflict")
            .count();
        let failing = repos
            .iter()
            .filter(|r| matches!(r.ci.as_ref().map(|c| c.state), Some(CiState::Failure)))
            .count();

        FolderView {
            path: base.display().to_string(),
            label,
            total: repos.len(),
            dirty,
            failing,
            missing: !base.exists(),
            repos,
        }
    }

    fn repo_view(&self, r: &RepoStatus) -> RepoView {
        let state = if r.error.is_some() {
            "error"
        } else if r.has_conflict() {
            "conflict"
        } else if r.is_uninitialized() {
            "empty"
        } else if r.is_clean() {
            "clean"
        } else {
            "dirty"
        };

        RepoView {
            name: r.name.clone(),
            path: r.path.display().to_string(),
            branch: r.branch.clone(),
            ahead: r.ahead,
            behind: r.behind,
            staged: r.staged,
            modified: r.modified,
            untracked: r.untracked,
            conflicted: r.conflicted,
            stashed: r.stashed,
            tag: r.latest_tag.as_ref().map(|t| t.name.clone()),
            commits_since_tag: r.latest_tag.as_ref().map(|t| t.commits_since).unwrap_or(0),
            last_commit: r.last_commit.as_ref().map(|c| c.summary.clone()),
            error: r.error.clone(),
            uninitialized: r.is_uninitialized(),
            pinned: self.pinned.contains(&r.path),
            ci: self.ci.get(&r.path).map(|c| CiView {
                state: c.state,
                name: c.name.clone(),
                url: c.url.clone(),
                failed_step: c.failed_step.clone(),
            }),
            state,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    fn paths(v: &[&str]) -> Vec<PathBuf> {
        v.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn folders_are_named_by_their_last_segment() {
        assert_eq!(
            super::label_folders(&paths(&["/Users/j/work", "/Users/j/personal"])),
            vec!["work", "personal"]
        );
    }

    #[test]
    fn two_folders_of_one_name_grow_until_they_differ() {
        assert_eq!(
            super::label_folders(&paths(&[
                "/Users/j/work/projects",
                "/Users/j/personal/projects"
            ])),
            vec!["work/projects", "personal/projects"]
        );
    }

    #[test]
    fn only_the_ones_that_clash_grow() {
        assert_eq!(
            super::label_folders(&paths(&["/a/x/projects", "/b/y/projects", "/c/notes"])),
            vec!["x/projects", "y/projects", "notes"]
        );
    }
}
