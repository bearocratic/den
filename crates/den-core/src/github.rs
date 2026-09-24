//! What `gh` and `git remote` can tell us about a repository.
//!
//! Every call here shells out and every one of them can fail on a
//! machine with no `gh`, no network, or no GitHub remote: the whole
//! module returns `Option`, never an error to surface.

use crate::model::{CiInfo, CiState, PrInfo};
use std::path::Path;
use std::process::{Command, Stdio};

pub fn gh_authed() -> bool {
    match Command::new("gh")
        .args(["auth", "status"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        Ok(s) => s.success(),
        Err(_) => false,
    }
}

pub fn detect_ci(path: &Path) -> Option<CiInfo> {
    let owner_repo = github_owner_repo(path)?;
    let commit = current_commit(path)?;
    let out = Command::new("gh")
        .args([
            "run",
            "list",
            "--repo",
            &owner_repo,
            "--commit",
            &commit,
            "--limit",
            "1",
            "--json",
            "status,conclusion,name,url",
            "-q",
            r#".[0] | [.status, .conclusion // "", .name, .url] | @tsv"#,
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        return None;
    }
    let parts: Vec<&str> = s.splitn(4, '\t').collect();
    if parts.len() < 4 {
        return None;
    }
    let state = ci_state_from(parts[0], parts[1])?;
    let url = parts[3].to_string();
    let failed_step = if state == CiState::Failure {
        run_id_from_url(&url).and_then(|id| failed_step(&owner_repo, &id))
    } else {
        None
    };
    Some(CiInfo {
        state,
        name: parts[2].to_string(),
        url,
        failed_step,
    })
}

pub fn run_id_from_url(url: &str) -> Option<String> {
    url.rsplit('/').next().map(|s| s.to_string())
}

pub fn failed_step(owner_repo: &str, run_id: &str) -> Option<String> {
    let out = Command::new("gh")
        .args([
            "run",
            "view",
            run_id,
            "--repo",
            owner_repo,
            "--json",
            "jobs",
            "-q",
            r#"[.jobs[] | select(.conclusion == "failure") | "\(.name) → \(.steps | map(select(.conclusion == "failure"))[0].name // "?")"] | .[0] // """#,
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

pub fn ci_state_from(status: &str, conclusion: &str) -> Option<CiState> {
    Some(match (status, conclusion) {
        ("completed", "success") => CiState::Success,
        ("completed", "failure" | "cancelled" | "timed_out" | "startup_failure") => {
            CiState::Failure
        }
        ("in_progress", _) | ("queued", _) | ("waiting", _) | ("requested", _) => {
            CiState::Running
        }
        _ => CiState::Unknown,
    })
}

pub fn github_owner_repo(path: &Path) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if let Some(rest) = url.strip_prefix("git@github.com:") {
        return Some(rest.strip_suffix(".git").unwrap_or(rest).to_string());
    }
    if let Some(rest) = url.strip_prefix("https://github.com/") {
        return Some(rest.strip_suffix(".git").unwrap_or(rest).to_string());
    }
    if let Some(rest) = url.strip_prefix("ssh://git@github.com/") {
        return Some(rest.strip_suffix(".git").unwrap_or(rest).to_string());
    }
    None
}

pub fn detect_prs(path: &Path) -> Option<Vec<PrInfo>> {
    let owner_repo = github_owner_repo(path)?;
    let out = Command::new("gh")
        .args([
            "pr",
            "list",
            "--repo",
            &owner_repo,
            "--author",
            "@me",
            "--state",
            "open",
            "--json",
            "number,title,url,createdAt",
            "-q",
            r#".[] | [(.number|tostring), .title, .url, .createdAt] | @tsv"#,
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let mut out = Vec::new();
    for line in s.lines() {
        let parts: Vec<&str> = line.splitn(4, '\t').collect();
        if parts.len() < 4 {
            continue;
        }
        let number: u64 = parts[0].parse().unwrap_or(0);
        out.push(PrInfo {
            number,
            title: parts[1].to_string(),
            url: parts[2].to_string(),
            age: relative_age(parts[3]),
        });
    }
    Some(out)
}

pub fn relative_age(iso: &str) -> String {
    let date_part = iso.split('T').next().unwrap_or(iso);
    date_part.to_string()
}

pub fn current_commit(path: &Path) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

pub fn git_remote_to_https(url: &str) -> Option<String> {
    let url = url.trim();
    if let Some(rest) = url.strip_prefix("https://") {
        let rest = rest.strip_suffix(".git").unwrap_or(rest);
        return Some(format!("https://{}", rest));
    }
    if let Some(rest) = url.strip_prefix("git@") {
        if let Some((host, path)) = rest.split_once(':') {
            let path = path.strip_suffix(".git").unwrap_or(path);
            return Some(format!("https://{}/{}", host, path));
        }
    }
    if let Some(rest) = url.strip_prefix("ssh://git@") {
        let rest = rest.strip_suffix(".git").unwrap_or(rest);
        let normalized = rest.replacen('/', ":", 1);
        if let Some((host, path)) = normalized.split_once(':') {
            return Some(format!("https://{}/{}", host, path));
        }
    }
    None
}

pub fn open_url(url: &str) {
    #[cfg(target_os = "macos")]
    let _ = Command::new("open")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    #[cfg(target_os = "linux")]
    let _ = Command::new("xdg-open")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let _ = url;
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remotes_become_browsable_urls() {
        assert_eq!(
            git_remote_to_https("git@github.com:bearocratic/den.git").as_deref(),
            Some("https://github.com/bearocratic/den")
        );
        assert_eq!(
            git_remote_to_https("ssh://git@github.com/bearocratic/den.git").as_deref(),
            Some("https://github.com/bearocratic/den")
        );
        assert_eq!(
            git_remote_to_https("https://github.com/bearocratic/den.git").as_deref(),
            Some("https://github.com/bearocratic/den")
        );
        // A remote we cannot browse stays unbrowsable rather than guessed at.
        assert_eq!(git_remote_to_https("/srv/git/den.git"), None);
    }

    #[test]
    fn a_run_url_yields_its_trailing_id() {
        assert_eq!(
            run_id_from_url("https://github.com/bearocratic/den/actions/runs/1234").as_deref(),
            Some("1234")
        );
    }

    #[test]
    fn ci_states_follow_the_conclusion_not_the_status() {
        assert_eq!(
            ci_state_from("completed", "success"),
            Some(CiState::Success)
        );
        assert_eq!(
            ci_state_from("completed", "failure"),
            Some(CiState::Failure)
        );
        assert_eq!(
            ci_state_from("completed", "timed_out"),
            Some(CiState::Failure)
        );
        assert_eq!(ci_state_from("in_progress", ""), Some(CiState::Running));
        assert_eq!(ci_state_from("queued", ""), Some(CiState::Running));
        // A conclusion we do not model is unknown, never quietly green.
        assert_eq!(
            ci_state_from("completed", "neutral"),
            Some(CiState::Unknown)
        );
    }
}
