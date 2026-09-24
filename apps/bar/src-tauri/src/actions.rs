//! Opening a repository somewhere else.
//!
//! Phase one of the bar reads and opens; nothing here writes to a
//! repository, so the worst a misfire can do is raise a window.

use std::path::Path;
use std::process::Command;

/// Reveal the folder in Finder.
pub fn reveal(path: &Path) {
    let _ = Command::new("open").arg(path).spawn();
}

/// Open a shell there, in whichever terminal the person uses.
pub fn terminal(path: &Path) {
    let term = std::env::var("DEN_TERMINAL").unwrap_or_else(|_| "Terminal".to_string());
    let _ = Command::new("open").arg("-a").arg(term).arg(path).spawn();
}

/// Open the folder in $EDITOR's app, defaulting to what most people have.
pub fn editor(path: &Path) {
    let app = std::env::var("DEN_EDITOR_APP").unwrap_or_else(|_| "Visual Studio Code".to_string());
    let _ = Command::new("open").arg("-a").arg(app).arg(path).spawn();
}

/// Open the repository on its host, if it has one we can browse.
pub fn github(path: &Path) -> bool {
    let Some(url) = remote_url(path) else {
        return false;
    };
    den_core::github::open_url(&url);
    true
}

/// Open a CI run, a pull request — anything the panel already has a URL for.
pub fn url(url: &str) {
    if url.starts_with("https://") {
        den_core::github::open_url(url);
    }
}

fn remote_url(path: &Path) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    den_core::github::git_remote_to_https(&String::from_utf8_lossy(&out.stdout))
}
