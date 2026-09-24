//! One ordering for every surface.
//!
//! The TUI and the menu bar must agree on which repository comes
//! first, so the rule lives here and both pass it the same view of
//! what is pinned, hidden, filtered and sorted.

use crate::model::SortMode;
use crate::repo::RepoStatus;
use std::collections::HashSet;
use std::path::PathBuf;

/// Everything the ordering needs, borrowed from whichever surface asks.
pub struct OrderView<'a> {
    pub repos: &'a [RepoStatus],
    pub filter_query: &'a str,
    pub show_hidden: bool,
    pub hidden: &'a HashSet<PathBuf>,
    pub pinned: &'a HashSet<PathBuf>,
    pub bases: &'a [PathBuf],
    pub base_filter: Option<usize>,
    pub sort_mode: SortMode,
}

pub fn display_order(v: &OrderView) -> Vec<usize> {
    let q = v.filter_query.trim().to_lowercase();
    let mut idx: Vec<usize> = (0..v.repos.len())
        .filter(|i| {
            let r = &v.repos[*i];
            if !v.show_hidden && v.hidden.contains(&r.path) {
                return false;
            }
            if !q.is_empty() && !r.name.to_lowercase().contains(&q) {
                return false;
            }
            if let Some(bi) = v.base_filter {
                if let Some(base) = v.bases.get(bi) {
                    if !r.path.starts_with(base) {
                        return false;
                    }
                }
            }
            true
        })
        .collect();
    idx.sort_by(|&a, &b| {
        let ra = &v.repos[a];
        let rb = &v.repos[b];
        let ha = v.hidden.contains(&ra.path);
        let hb = v.hidden.contains(&rb.path);
        if ha != hb {
            return ha.cmp(&hb);
        }
        let pa = v.pinned.contains(&ra.path);
        let pb = v.pinned.contains(&rb.path);
        if pa != pb {
            return pb.cmp(&pa);
        }
        match v.sort_mode {
            SortMode::ByRecency => {
                let ta = ra.last_commit.as_ref().map(|c| c.time);
                let tb = rb.last_commit.as_ref().map(|c| c.time);
                if ta != tb {
                    return tb.cmp(&ta);
                }
            }
            // Default, CiRedFirst and DirtyFirst all lead with state;
            // CI redness reorders the sections, not the repos.
            _ => {
                let pa = state_priority(ra);
                let pb = state_priority(rb);
                if pa != pb {
                    return pa.cmp(&pb);
                }
            }
        }
        ra.name.cmp(&rb.name)
    });
    idx
}

pub fn state_priority(r: &RepoStatus) -> u8 {
    if r.has_conflict() || r.error.is_some() {
        0
    } else if r.is_uninitialized() {
        3
    } else if !r.is_clean() {
        1
    } else {
        2
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::RepoStatus;

    fn repo(name: &str, path: &str) -> RepoStatus {
        RepoStatus {
            path: PathBuf::from(path),
            name: name.to_string(),
            branch: Some("main".into()),
            upstream: Some("origin/main".into()),
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
        }
    }

    fn view<'a>(
        repos: &'a [RepoStatus],
        hidden: &'a HashSet<PathBuf>,
        pinned: &'a HashSet<PathBuf>,
        bases: &'a [PathBuf],
    ) -> OrderView<'a> {
        OrderView {
            repos,
            filter_query: "",
            show_hidden: false,
            hidden,
            pinned,
            bases,
            base_filter: None,
            sort_mode: SortMode::Default,
        }
    }

    fn names(repos: &[RepoStatus], order: Vec<usize>) -> Vec<&str> {
        order.iter().map(|i| repos[*i].name.as_str()).collect()
    }

    #[test]
    fn trouble_comes_first_and_clean_repos_last() {
        let mut conflicted = repo("conflicted", "/w/conflicted");
        conflicted.conflicted = 1;
        let mut dirty = repo("dirty", "/w/dirty");
        dirty.modified = 2;
        let repos = vec![repo("clean", "/w/clean"), dirty, conflicted];
        let (h, p, b) = (HashSet::new(), HashSet::new(), Vec::new());
        assert_eq!(
            names(&repos, display_order(&view(&repos, &h, &p, &b))),
            vec!["conflicted", "dirty", "clean"]
        );
    }

    #[test]
    fn a_pin_outranks_state_and_hiding_removes_the_row() {
        let mut dirty = repo("dirty", "/w/dirty");
        dirty.modified = 1;
        let repos = vec![dirty, repo("pinned", "/w/pinned"), repo("gone", "/w/gone")];
        let hidden: HashSet<PathBuf> = [PathBuf::from("/w/gone")].into_iter().collect();
        let pinned: HashSet<PathBuf> = [PathBuf::from("/w/pinned")].into_iter().collect();
        assert_eq!(
            names(&repos, display_order(&view(&repos, &hidden, &pinned, &[]))),
            vec!["pinned", "dirty"]
        );
    }

    #[test]
    fn show_hidden_puts_the_hidden_rows_back_at_the_end() {
        let repos = vec![repo("gone", "/w/gone"), repo("here", "/w/here")];
        let hidden: HashSet<PathBuf> = [PathBuf::from("/w/gone")].into_iter().collect();
        let pinned = HashSet::new();
        let mut v = view(&repos, &hidden, &pinned, &[]);
        v.show_hidden = true;
        assert_eq!(names(&repos, display_order(&v)), vec!["here", "gone"]);
    }

    #[test]
    fn the_filter_matches_names_and_the_base_filter_matches_paths() {
        let repos = vec![
            repo("cabinet", "/work/cabinet"),
            repo("chancery", "/work/chancery"),
            repo("notes", "/personal/notes"),
        ];
        let (h, p) = (HashSet::new(), HashSet::new());
        let bases = vec![PathBuf::from("/work"), PathBuf::from("/personal")];

        let mut v = view(&repos, &h, &p, &bases);
        v.filter_query = "chan";
        assert_eq!(names(&repos, display_order(&v)), vec!["chancery"]);

        let mut v = view(&repos, &h, &p, &bases);
        v.base_filter = Some(1);
        assert_eq!(names(&repos, display_order(&v)), vec!["notes"]);
    }

    #[test]
    fn dirty_first_orders_the_same_way_the_default_does() {
        let mut dirty = repo("dirty", "/w/dirty");
        dirty.modified = 1;
        let repos = vec![repo("clean", "/w/clean"), dirty];
        let (h, p) = (HashSet::new(), HashSet::new());
        let mut v = view(&repos, &h, &p, &[]);
        v.sort_mode = SortMode::DirtyFirst;
        let dirty_first = display_order(&v);
        let default = display_order(&view(&repos, &h, &p, &[]));
        assert_eq!(dirty_first, default);
        assert_eq!(names(&repos, default), vec!["dirty", "clean"]);
    }
}
