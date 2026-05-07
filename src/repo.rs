use git2::{Repository, StatusOptions};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct RepoStatus {
    pub path: PathBuf,
    pub name: String,
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: usize,
    pub behind: usize,
    pub staged: usize,
    pub modified: usize,
    pub untracked: usize,
    pub conflicted: usize,
    pub stashed: usize,
    pub last_commit: Option<CommitInfo>,
    pub latest_tag: Option<TagInfo>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub short_sha: String,
    pub summary: String,
    pub time: SystemTime,
}

#[derive(Debug, Clone)]
pub struct TagInfo {
    pub name: String,
    pub commits_since: usize,
    pub time: SystemTime,
}

impl RepoStatus {
    pub fn is_clean(&self) -> bool {
        self.error.is_none()
            && self.staged == 0
            && self.modified == 0
            && self.untracked == 0
            && self.conflicted == 0
    }

    pub fn has_conflict(&self) -> bool {
        self.conflicted > 0
    }

    pub fn is_uninitialized(&self) -> bool {
        self.error.is_none() && self.last_commit.is_none() && self.branch.is_none()
    }
}

const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".venv",
    "venv",
    "vendor",
    ".git",
    ".cache",
];

pub fn discover(base: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut repos = Vec::new();
    let mut walker = WalkDir::new(base)
        .max_depth(max_depth)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            if !e.file_type().is_dir() {
                return true;
            }
            let n = e.file_name().to_string_lossy();
            !SKIP_DIRS.contains(&n.as_ref())
        });

    loop {
        let entry = match walker.next() {
            Some(Ok(e)) => e,
            Some(Err(_)) => continue,
            None => break,
        };
        if !entry.file_type().is_dir() {
            continue;
        }
        let path = entry.path();
        if path.join(".git").exists() {
            repos.push(path.to_path_buf());
            walker.skip_current_dir();
        }
    }

    repos.sort();
    repos
}

pub fn status_for(path: &Path) -> RepoStatus {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("?")
        .to_string();
    let mut s = RepoStatus {
        path: path.to_path_buf(),
        name,
        branch: None,
        upstream: None,
        ahead: 0,
        behind: 0,
        staged: 0,
        modified: 0,
        untracked: 0,
        conflicted: 0,
        stashed: 0,
        last_commit: None,
        latest_tag: None,
        error: None,
    };

    let mut repo = match Repository::open(path) {
        Ok(r) => r,
        Err(e) => {
            s.error = Some(format!("open: {}", e.message()));
            return s;
        }
    };

    if let Ok(head) = repo.head() {
        if head.is_branch() {
            if let Some(name) = head.shorthand() {
                s.branch = Some(name.to_string());
            }
        } else if let Some(name) = head.shorthand() {
            s.branch = Some(format!("({})", name));
        }
        if let Ok(commit) = head.peel_to_commit() {
            let short: String = commit.id().to_string().chars().take(7).collect();
            let summary = commit.summary().unwrap_or("").to_string();
            let secs = commit.time().seconds().max(0) as u64;
            let time = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs);
            s.last_commit = Some(CommitInfo {
                short_sha: short,
                summary,
                time,
            });
        }
    }

    if let Some(branch_name) = s.branch.clone() {
        if let Ok(branch) = repo.find_branch(&branch_name, git2::BranchType::Local) {
            if let Ok(upstream) = branch.upstream() {
                if let Ok(Some(n)) = upstream.name() {
                    s.upstream = Some(n.to_string());
                }
                if let (Some(l), Some(u)) = (branch.get().target(), upstream.get().target()) {
                    if let Ok((ahead, behind)) = repo.graph_ahead_behind(l, u) {
                        s.ahead = ahead;
                        s.behind = behind;
                    }
                }
            }
        }
    }

    let mut opts = StatusOptions::new();
    opts.include_untracked(true).recurse_untracked_dirs(true);
    if let Ok(statuses) = repo.statuses(Some(&mut opts)) {
        for entry in statuses.iter() {
            let st = entry.status();
            if st.is_conflicted() {
                s.conflicted += 1;
                continue;
            }
            if st.is_index_new()
                || st.is_index_modified()
                || st.is_index_deleted()
                || st.is_index_renamed()
                || st.is_index_typechange()
            {
                s.staged += 1;
            }
            if st.is_wt_modified()
                || st.is_wt_deleted()
                || st.is_wt_renamed()
                || st.is_wt_typechange()
            {
                s.modified += 1;
            }
            if st.is_wt_new() {
                s.untracked += 1;
            }
        }
    }

    let mut stash_count = 0usize;
    let _ = repo.stash_foreach(|_idx, _msg, _oid| {
        stash_count += 1;
        true
    });
    s.stashed = stash_count;

    s.latest_tag = latest_tag(path);

    s
}

fn latest_tag(path: &Path) -> Option<TagInfo> {
    let out = Command::new("git")
        .arg("-C")
        .arg(path)
        .args([
            "for-each-ref",
            "--sort=-creatordate",
            "--format=%(refname:short)\x1f%(creatordate:unix)",
            "refs/tags",
            "--count=1",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&out.stdout);
    let line = raw.lines().next()?;
    let mut parts = line.splitn(2, '\x1f');
    let name = parts.next()?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    let secs: u64 = parts.next().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    let time = SystemTime::UNIX_EPOCH + Duration::from_secs(secs);

    let commits_since = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-list", "--count", &format!("{}..HEAD", name)])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8_lossy(&o.stdout).trim().parse().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);

    Some(TagInfo {
        name,
        commits_since,
        time,
    })
}
