mod brand;
mod markdown;
mod repo;
mod ui;

use anyhow::Result;
use clap::Parser;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode, DebounceEventResult};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::collections::HashSet;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::repo::{discover, status_for, RepoStatus};

#[derive(Parser, Debug)]
#[command(name = "den", version, about = "TUI watcher for dirty git repos")]
struct Args {
    /// Base folder to scan. Defaults to current directory.
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Maximum recursion depth when scanning for repos.
    #[arg(long, default_value_t = 4)]
    depth: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailSection {
    Status,
    Diff,
    History,
    Releases,
}

impl DetailSection {
    pub fn next(self) -> Self {
        match self {
            DetailSection::Status => DetailSection::Diff,
            DetailSection::Diff => DetailSection::History,
            DetailSection::History => DetailSection::Releases,
            DetailSection::Releases => DetailSection::Status,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            DetailSection::Status => "status",
            DetailSection::Diff => "diff",
            DetailSection::History => "history",
            DetailSection::Releases => "releases",
        }
    }
}

pub struct App {
    pub repos: Vec<RepoStatus>,
    pub selected: usize,
    pub show_detail: bool,
    pub base: PathBuf,
    pub cols: usize,
    pub last_refresh: Instant,
    pub focus: DetailSection,
    pub detail_cache_key: Option<PathBuf>,
    pub status_content: String,
    pub diff_content: String,
    pub history_content: String,
    pub release_tag: String,
    pub release_time_unix: u64,
    pub release_subject: String,
    pub release_body: String,
    pub release_notes_path: String,
    pub release_notes_content: String,
    pub releases_rendered: bool,
    pub readme_content: String,
    pub readme_path: String,
    pub status_scroll: u16,
    pub diff_scroll: u16,
    pub history_scroll: u16,
    pub releases_scroll: u16,
    pub readme_scroll: u16,
    pub show_readme: bool,
    pub readme_rendered: bool,
    pub pinned: HashSet<PathBuf>,
    pub hidden: HashSet<PathBuf>,
    pub show_hidden: bool,
    pub palette_open: bool,
    pub palette_query: String,
    pub palette_selected: usize,
    pub pending_lazygit: Option<PathBuf>,
    pub flash: Option<(String, Instant)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmd {
    Pin,
    Hide,
    ToggleShowHidden,
    OpenEditor,
    OpenLazyGit,
    OpenGitHub,
    CopyPath,
    RefreshAll,
    ToggleReadme,
    ToggleMarkdownMode,
    ToggleDetail,
    FocusStatus,
    FocusDiff,
    FocusHistory,
    FocusReleases,
    Quit,
}

pub struct CmdInfo {
    pub cmd: Cmd,
    pub name: &'static str,
    pub keys: &'static str,
    pub desc: &'static str,
}

pub fn all_commands() -> &'static [CmdInfo] {
    &[
        CmdInfo {
            cmd: Cmd::Pin,
            name: "pin / unpin",
            keys: "p",
            desc: "toggle pin on the selected repo",
        },
        CmdInfo {
            cmd: Cmd::Hide,
            name: "hide / unhide",
            keys: "x",
            desc: "hide selected repo from the grid",
        },
        CmdInfo {
            cmd: Cmd::ToggleShowHidden,
            name: "show hidden",
            keys: ".",
            desc: "toggle visibility of hidden repos",
        },
        CmdInfo {
            cmd: Cmd::OpenEditor,
            name: "open in editor",
            keys: "e",
            desc: "open repo in $VISUAL / $EDITOR (or VS Code)",
        },
        CmdInfo {
            cmd: Cmd::OpenLazyGit,
            name: "open in lazygit",
            keys: "o",
            desc: "launch lazygit on the repo (suspends den)",
        },
        CmdInfo {
            cmd: Cmd::OpenGitHub,
            name: "open on GitHub",
            keys: "g",
            desc: "open the repo's origin URL in browser",
        },
        CmdInfo {
            cmd: Cmd::CopyPath,
            name: "copy path",
            keys: "y",
            desc: "copy the repo path to clipboard",
        },
        CmdInfo {
            cmd: Cmd::RefreshAll,
            name: "refresh all",
            keys: "r",
            desc: "re-scan every repo's status",
        },
        CmdInfo {
            cmd: Cmd::ToggleDetail,
            name: "toggle detail",
            keys: "↵",
            desc: "open or close the detail pane",
        },
        CmdInfo {
            cmd: Cmd::ToggleReadme,
            name: "readme",
            keys: "i",
            desc: "open the README overlay",
        },
        CmdInfo {
            cmd: Cmd::ToggleMarkdownMode,
            name: "toggle raw / rendered",
            keys: "m",
            desc: "switch markdown view mode (readme + releases)",
        },
        CmdInfo {
            cmd: Cmd::FocusStatus,
            name: "focus status",
            keys: "1",
            desc: "focus the status section in detail",
        },
        CmdInfo {
            cmd: Cmd::FocusDiff,
            name: "focus diff",
            keys: "2",
            desc: "focus the diff section in detail",
        },
        CmdInfo {
            cmd: Cmd::FocusHistory,
            name: "focus history",
            keys: "3",
            desc: "focus the history section in detail",
        },
        CmdInfo {
            cmd: Cmd::FocusReleases,
            name: "focus releases",
            keys: "4",
            desc: "focus the releases section in detail",
        },
        CmdInfo {
            cmd: Cmd::Quit,
            name: "quit",
            keys: "q",
            desc: "exit den",
        },
    ]
}

pub fn filter_commands(query: &str) -> Vec<&'static CmdInfo> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return all_commands().iter().collect();
    }
    all_commands()
        .iter()
        .filter(|c| {
            c.name.to_lowercase().contains(&q)
                || c.desc.to_lowercase().contains(&q)
                || c.keys.to_lowercase().contains(&q)
        })
        .collect()
}

impl App {
    pub fn focused_scroll_mut(&mut self) -> &mut u16 {
        if self.show_readme {
            return &mut self.readme_scroll;
        }
        match self.focus {
            DetailSection::Status => &mut self.status_scroll,
            DetailSection::Diff => &mut self.diff_scroll,
            DetailSection::History => &mut self.history_scroll,
            DetailSection::Releases => &mut self.releases_scroll,
        }
    }
    pub fn flash_msg(&mut self, s: impl Into<String>) {
        self.flash = Some((s.into(), Instant::now()));
    }
}

pub fn display_order(app: &App) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..app.repos.len())
        .filter(|i| {
            let r = &app.repos[*i];
            app.show_hidden || !app.hidden.contains(&r.path)
        })
        .collect();
    idx.sort_by(|&a, &b| {
        let ra = &app.repos[a];
        let rb = &app.repos[b];
        let ha = app.hidden.contains(&ra.path);
        let hb = app.hidden.contains(&rb.path);
        if ha != hb {
            return ha.cmp(&hb);
        }
        let pa = app.pinned.contains(&ra.path);
        let pb = app.pinned.contains(&rb.path);
        if pa != pb {
            return pb.cmp(&pa);
        }
        let prio_a = state_priority(ra);
        let prio_b = state_priority(rb);
        if prio_a != prio_b {
            return prio_a.cmp(&prio_b);
        }
        ra.name.cmp(&rb.name)
    });
    idx
}

fn state_priority(r: &RepoStatus) -> u8 {
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

fn main() -> Result<()> {
    let args = Args::parse();
    let base = args.path.canonicalize()?;

    eprintln!("scanning {} (depth {})…", base.display(), args.depth);
    let repo_paths = discover(&base, args.depth);
    if repo_paths.is_empty() {
        eprintln!("no git repos found within depth {}", args.depth);
        return Ok(());
    }
    eprintln!("found {} repos", repo_paths.len());

    let mut statuses: Vec<RepoStatus> = repo_paths.iter().map(|p| status_for(p)).collect();
    statuses.sort_by(|a, b| a.name.cmp(&b.name));

    let pinned = load_pin_set("pins.txt");
    let hidden = load_pin_set("hidden.txt");

    let (tx, rx) = mpsc::channel::<DebounceEventResult>();
    let mut debouncer = new_debouncer(Duration::from_millis(400), move |res| {
        let _ = tx.send(res);
    })?;
    for p in &repo_paths {
        let _ = debouncer.watcher().watch(p, RecursiveMode::Recursive);
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App {
        repos: statuses,
        selected: 0,
        show_detail: true,
        base: base.clone(),
        cols: 1,
        last_refresh: Instant::now(),
        focus: DetailSection::Status,
        detail_cache_key: None,
        status_content: String::new(),
        diff_content: String::new(),
        history_content: String::new(),
        release_tag: String::new(),
        release_time_unix: 0,
        release_subject: String::new(),
        release_body: String::new(),
        release_notes_path: String::new(),
        release_notes_content: String::new(),
        releases_rendered: true,
        readme_content: String::new(),
        readme_path: String::new(),
        status_scroll: 0,
        diff_scroll: 0,
        history_scroll: 0,
        releases_scroll: 0,
        readme_scroll: 0,
        show_readme: false,
        readme_rendered: true,
        pinned,
        hidden,
        show_hidden: false,
        palette_open: false,
        palette_query: String::new(),
        palette_selected: 0,
        pending_lazygit: None,
        flash: None,
    };

    let res = run(&mut terminal, &mut app, &rx);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    res
}

fn run<B: ratatui::backend::Backend + std::io::Write>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    rx: &mpsc::Receiver<DebounceEventResult>,
) -> Result<()>
where
    <B as ratatui::backend::Backend>::Error: Send + Sync + 'static,
{
    loop {
        ensure_detail(app);
        terminal.draw(|f| ui::render(f, app))?;

        if event::poll(Duration::from_millis(150))? {
            if let Event::Key(k) = event::read()? {
                if k.kind != KeyEventKind::Press {
                    continue;
                }
                let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
                if app.palette_open {
                    match k.code {
                        KeyCode::Char('c') if ctrl => return Ok(()),
                        KeyCode::Esc => {
                            app.palette_open = false;
                            app.palette_query.clear();
                            app.palette_selected = 0;
                        }
                        KeyCode::Enter => {
                            let matches = filter_commands(&app.palette_query);
                            let cmd = matches.get(app.palette_selected).map(|c| c.cmd);
                            app.palette_open = false;
                            app.palette_query.clear();
                            app.palette_selected = 0;
                            if let Some(c) = cmd {
                                if let Some(quit) = execute_cmd(app, c) {
                                    if quit {
                                        return Ok(());
                                    }
                                }
                            }
                        }
                        KeyCode::Up => {
                            app.palette_selected = app.palette_selected.saturating_sub(1);
                        }
                        KeyCode::Down => {
                            let n = filter_commands(&app.palette_query).len();
                            if n > 0 && app.palette_selected + 1 < n {
                                app.palette_selected += 1;
                            }
                        }
                        KeyCode::Backspace => {
                            app.palette_query.pop();
                            app.palette_selected = 0;
                        }
                        KeyCode::Char(c) => {
                            app.palette_query.push(c);
                            app.palette_selected = 0;
                        }
                        _ => {}
                    }
                    continue;
                }
                match k.code {
                    KeyCode::Char(':') => {
                        app.palette_open = true;
                        app.palette_query.clear();
                        app.palette_selected = 0;
                    }
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Char('c') if ctrl => return Ok(()),
                    KeyCode::Char('r') => {
                        refresh_all(app);
                        app.detail_cache_key = None;
                    }
                    KeyCode::Left | KeyCode::Char('h') => move_sel(app, -1, 0),
                    KeyCode::Right | KeyCode::Char('l') => move_sel(app, 1, 0),
                    KeyCode::Up | KeyCode::Char('k') => move_sel(app, 0, -1),
                    KeyCode::Down | KeyCode::Char('j') => move_sel(app, 0, 1),
                    KeyCode::Enter => {
                        app.show_detail = !app.show_detail;
                    }
                    KeyCode::Tab => {
                        if app.show_detail {
                            app.focus = app.focus.next();
                        } else {
                            app.show_detail = true;
                        }
                    }
                    KeyCode::Char('1') => {
                        app.show_detail = true;
                        app.show_readme = false;
                        app.focus = DetailSection::Status;
                    }
                    KeyCode::Char('2') => {
                        app.show_detail = true;
                        app.show_readme = false;
                        app.focus = DetailSection::Diff;
                    }
                    KeyCode::Char('3') => {
                        app.show_detail = true;
                        app.show_readme = false;
                        app.focus = DetailSection::History;
                    }
                    KeyCode::Char('4') => {
                        app.show_detail = true;
                        app.show_readme = false;
                        app.focus = DetailSection::Releases;
                    }
                    KeyCode::Char('i') => {
                        app.show_detail = true;
                        app.show_readme = !app.show_readme;
                    }
                    KeyCode::Char('m') => {
                        if app.show_readme {
                            app.readme_rendered = !app.readme_rendered;
                            app.readme_scroll = 0;
                        } else if app.show_detail && app.focus == DetailSection::Releases {
                            app.releases_rendered = !app.releases_rendered;
                            app.releases_scroll = 0;
                        }
                    }
                    KeyCode::Char('x') => toggle_hidden(app),
                    KeyCode::Char('.') => {
                        app.show_hidden = !app.show_hidden;
                        app.flash_msg(if app.show_hidden {
                            "showing hidden repos"
                        } else {
                            "hiding muted repos"
                        });
                    }
                    KeyCode::PageDown => {
                        let s = app.focused_scroll_mut();
                        *s = s.saturating_add(10);
                    }
                    KeyCode::PageUp => {
                        let s = app.focused_scroll_mut();
                        *s = s.saturating_sub(10);
                    }
                    KeyCode::Char('p') => toggle_pin(app),
                    KeyCode::Char('e') => action_editor(app),
                    KeyCode::Char('o') => action_lazygit(app),
                    KeyCode::Char('g') => action_github(app),
                    KeyCode::Char('y') => action_copy_path(app),
                    _ => {}
                }
            }
        }

        let mut changed: Vec<PathBuf> = Vec::new();
        while let Ok(res) = rx.try_recv() {
            if let Ok(events) = res {
                for ev in events {
                    if let Some(repo_root) = repo_for_path(app, &ev.path) {
                        if !changed.iter().any(|p| p == &repo_root) {
                            changed.push(repo_root);
                        }
                    }
                }
            }
        }
        let selected_path = app.repos.get(app.selected).map(|r| r.path.clone());
        for p in &changed {
            if let Some(idx) = app.repos.iter().position(|r| &r.path == p) {
                app.repos[idx] = status_for(p);
            }
        }
        if let Some(sel) = &selected_path {
            if changed.iter().any(|p| p == sel) {
                app.detail_cache_key = None;
            }
        }

        if app.last_refresh.elapsed() > Duration::from_secs(30) {
            refresh_all(app);
        }

        if let Some((_, t)) = &app.flash {
            if t.elapsed() > Duration::from_secs(2) {
                app.flash = None;
            }
        }

        if let Some(path) = app.pending_lazygit.take() {
            disable_raw_mode()?;
            execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
            let status = Command::new("lazygit").arg("-p").arg(&path).status();
            execute!(terminal.backend_mut(), EnterAlternateScreen)?;
            enable_raw_mode()?;
            terminal.clear()?;
            match status {
                Ok(_) => app.flash_msg("returned from lazygit"),
                Err(e) => app.flash_msg(format!("lazygit failed: {}", e)),
            }
            refresh_all(app);
            app.detail_cache_key = None;
        }
    }
}

fn refresh_all(app: &mut App) {
    for r in &mut app.repos {
        *r = status_for(&r.path);
    }
    app.last_refresh = Instant::now();
}

fn repo_for_path(app: &App, p: &Path) -> Option<PathBuf> {
    app.repos
        .iter()
        .find(|r| p.starts_with(&r.path))
        .map(|r| r.path.clone())
}

fn move_sel(app: &mut App, dx: i32, dy: i32) {
    let order = display_order(app);
    if order.is_empty() {
        return;
    }
    let cur_pos = order
        .iter()
        .position(|&i| i == app.selected)
        .unwrap_or(0) as i32;
    let cols = app.cols.max(1) as i32;
    let mut pos = cur_pos + dx + dy * cols;
    if pos < 0 {
        pos = 0;
    }
    if pos >= order.len() as i32 {
        pos = order.len() as i32 - 1;
    }
    app.selected = order[pos as usize];
}

fn ensure_detail(app: &mut App) {
    if !app.show_detail {
        return;
    }
    let Some(repo) = app.repos.get(app.selected) else {
        return;
    };
    if app.detail_cache_key.as_ref() == Some(&repo.path) {
        return;
    }
    let path = repo.path.clone();
    app.status_content = fetch_status(&path);
    app.diff_content = fetch_diff(&path);
    app.history_content = fetch_history(&path);
    let r = fetch_release(&path);
    app.release_tag = r.tag;
    app.release_time_unix = r.time_unix;
    app.release_subject = r.subject;
    app.release_body = r.body;
    app.release_notes_path = r.notes_path;
    app.release_notes_content = r.notes_content;
    let (readme_path, readme_content) = fetch_readme(&path);
    app.readme_path = readme_path;
    app.readme_content = readme_content;
    app.status_scroll = 0;
    app.diff_scroll = 0;
    app.history_scroll = 0;
    app.releases_scroll = 0;
    app.readme_scroll = 0;
    app.detail_cache_key = Some(path);
}

struct LatestRelease {
    tag: String,
    time_unix: u64,
    subject: String,
    body: String,
    notes_path: String,
    notes_content: String,
}

fn run_git(path: &Path, args: &[&str]) -> String {
    match Command::new("git").arg("-C").arg(path).args(args).output() {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).into_owned(),
        Ok(out) => format!(
            "git {} failed:\n{}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        ),
        Err(e) => format!("failed to run git: {}", e),
    }
}

fn fetch_status(path: &Path) -> String {
    run_git(path, &["status", "--porcelain=v1", "-b"])
}

fn fetch_diff(path: &Path) -> String {
    let s = run_git(path, &["diff", "HEAD", "--no-color", "--stat=200", "--patch"]);
    if s.trim().is_empty() {
        run_git(path, &["diff", "--no-color", "--cached", "--patch"])
    } else {
        s
    }
}

fn fetch_history(path: &Path) -> String {
    run_git(
        path,
        &[
            "log",
            "-n",
            "30",
            "--no-color",
            "--pretty=format:%h\x1f%ar\x1f%an\x1f%s",
        ],
    )
}

fn fetch_readme(path: &Path) -> (String, String) {
    let candidates = [
        "README.md",
        "Readme.md",
        "readme.md",
        "README.markdown",
        "README.rst",
        "README.txt",
        "README",
    ];
    for c in candidates {
        let p = path.join(c);
        if p.is_file() {
            if let Ok(s) = std::fs::read_to_string(&p) {
                return (c.to_string(), s);
            }
        }
    }
    (String::new(), String::new())
}

fn fetch_release(path: &Path) -> LatestRelease {
    let mut out = LatestRelease {
        tag: String::new(),
        time_unix: 0,
        subject: String::new(),
        body: String::new(),
        notes_path: String::new(),
        notes_content: String::new(),
    };
    let raw = run_git(
        path,
        &[
            "for-each-ref",
            "--sort=-creatordate",
            "--format=%(refname:short)\x1f%(creatordate:unix)\x1f%(contents:subject)\x1f%(contents:body)",
            "refs/tags",
            "--count=1",
        ],
    );
    if raw.trim().is_empty() || raw.starts_with("git ") {
        return out;
    }
    let line = raw.lines().next().unwrap_or("");
    let parts: Vec<&str> = line.splitn(4, '\x1f').collect();
    if parts.is_empty() || parts[0].trim().is_empty() {
        return out;
    }
    let name = parts[0].trim().to_string();
    out.time_unix = parts
        .get(1)
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    out.subject = parts.get(2).copied().unwrap_or("").to_string();
    out.body = parts.get(3).copied().unwrap_or("").to_string();

    let candidates = [
        format!("releases/{}.md", name),
        format!("releases/{}/README.md", name),
        format!("releases/{}/index.md", name),
        format!("releases/{}/notes.md", name),
        format!("releases/{}/CHANGELOG.md", name),
        "CHANGELOG.md".to_string(),
        "RELEASES.md".to_string(),
    ];
    for c in &candidates {
        let p = path.join(c);
        if p.is_file() {
            if let Ok(s) = std::fs::read_to_string(&p) {
                out.notes_path = c.clone();
                out.notes_content = s;
                break;
            }
        }
    }
    out.tag = name;
    out
}

fn config_path(name: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".config/den").join(name)
}

fn load_pin_set(name: &str) -> HashSet<PathBuf> {
    let p = config_path(name);
    let content = std::fs::read_to_string(&p).unwrap_or_default();
    content
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect()
}

fn save_pin_set(name: &str, set: &HashSet<PathBuf>) {
    let p = config_path(name);
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut entries: Vec<String> = set.iter().filter_map(|p| p.to_str().map(String::from)).collect();
    entries.sort();
    let _ = std::fs::write(&p, entries.join("\n"));
}

fn toggle_pin(app: &mut App) {
    let Some(repo) = app.repos.get(app.selected) else {
        return;
    };
    let path = repo.path.clone();
    let name = repo.name.clone();
    if app.pinned.contains(&path) {
        app.pinned.remove(&path);
        app.flash_msg(format!("unpinned {}", name));
    } else {
        app.pinned.insert(path);
        app.flash_msg(format!("pinned {}", name));
    }
    save_pin_set("pins.txt", &app.pinned);
}

fn toggle_hidden(app: &mut App) {
    let Some(repo) = app.repos.get(app.selected) else {
        return;
    };
    let path = repo.path.clone();
    let name = repo.name.clone();
    if app.hidden.contains(&path) {
        app.hidden.remove(&path);
        app.flash_msg(format!("unhid {}", name));
    } else {
        app.hidden.insert(path.clone());
        app.flash_msg(format!("hid {}  ·  press . to toggle hidden", name));
        if !app.show_hidden {
            let order = display_order(app);
            if !order.is_empty() {
                let cur = order.iter().position(|&i| i == app.selected);
                if cur.is_none() {
                    app.selected = order[0];
                }
            }
        }
    }
    save_pin_set("hidden.txt", &app.hidden);
}

fn action_editor(app: &mut App) {
    let Some(repo) = app.repos.get(app.selected) else {
        return;
    };
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "code".to_string());
    let res = Command::new(&editor)
        .arg(&repo.path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    match res {
        Ok(_) => app.flash_msg(format!("opened in {}", editor)),
        Err(e) => app.flash_msg(format!("editor failed: {}", e)),
    }
}

fn action_lazygit(app: &mut App) {
    if let Some(repo) = app.repos.get(app.selected) {
        app.pending_lazygit = Some(repo.path.clone());
    }
}

fn action_github(app: &mut App) {
    let Some(repo) = app.repos.get(app.selected) else {
        return;
    };
    let out = Command::new("git")
        .arg("-C")
        .arg(&repo.path)
        .args(["remote", "get-url", "origin"])
        .output();
    let url = match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => {
            app.flash_msg("no origin remote");
            return;
        }
    };
    let Some(http_url) = git_remote_to_https(&url) else {
        app.flash_msg("could not parse remote URL");
        return;
    };
    open_url(&http_url);
    app.flash_msg(format!("opened {}", http_url));
}

fn git_remote_to_https(url: &str) -> Option<String> {
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

fn open_url(url: &str) {
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

fn action_copy_path(app: &mut App) {
    let Some(repo) = app.repos.get(app.selected) else {
        return;
    };
    let s = repo.path.to_string_lossy().into_owned();
    if copy_to_clipboard(&s) {
        app.flash_msg("path copied");
    } else {
        app.flash_msg("clipboard unavailable");
    }
}

fn execute_cmd(app: &mut App, cmd: Cmd) -> Option<bool> {
    match cmd {
        Cmd::Pin => toggle_pin(app),
        Cmd::Hide => toggle_hidden(app),
        Cmd::ToggleShowHidden => {
            app.show_hidden = !app.show_hidden;
            app.flash_msg(if app.show_hidden {
                "showing hidden repos"
            } else {
                "hiding muted repos"
            });
        }
        Cmd::OpenEditor => action_editor(app),
        Cmd::OpenLazyGit => action_lazygit(app),
        Cmd::OpenGitHub => action_github(app),
        Cmd::CopyPath => action_copy_path(app),
        Cmd::RefreshAll => {
            refresh_all(app);
            app.detail_cache_key = None;
            app.flash_msg("refreshed");
        }
        Cmd::ToggleDetail => {
            app.show_detail = !app.show_detail;
        }
        Cmd::ToggleReadme => {
            app.show_detail = true;
            app.show_readme = !app.show_readme;
        }
        Cmd::ToggleMarkdownMode => {
            if app.show_readme {
                app.readme_rendered = !app.readme_rendered;
                app.readme_scroll = 0;
            } else if app.show_detail && app.focus == DetailSection::Releases {
                app.releases_rendered = !app.releases_rendered;
                app.releases_scroll = 0;
            } else {
                app.flash_msg("open readme or focus releases first");
            }
        }
        Cmd::FocusStatus => {
            app.show_detail = true;
            app.show_readme = false;
            app.focus = DetailSection::Status;
        }
        Cmd::FocusDiff => {
            app.show_detail = true;
            app.show_readme = false;
            app.focus = DetailSection::Diff;
        }
        Cmd::FocusHistory => {
            app.show_detail = true;
            app.show_readme = false;
            app.focus = DetailSection::History;
        }
        Cmd::FocusReleases => {
            app.show_detail = true;
            app.show_readme = false;
            app.focus = DetailSection::Releases;
        }
        Cmd::Quit => return Some(true),
    }
    Some(false)
}

fn copy_to_clipboard(s: &str) -> bool {
    let cmds: &[(&str, &[&str])] = &[
        ("pbcopy", &[]),
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard"]),
        ("xsel", &["--clipboard", "--input"]),
    ];
    for (bin, args) in cmds {
        let mut cmd = Command::new(bin);
        cmd.args(*args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Ok(mut child) = cmd.spawn() {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(s.as_bytes());
            }
            if child.wait().is_ok() {
                return true;
            }
        }
    }
    false
}
