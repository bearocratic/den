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
    /// Base folder(s) to scan. Defaults to current directory.
    #[arg(default_value = ".")]
    paths: Vec<PathBuf>,

    /// Maximum recursion depth when scanning for repos.
    #[arg(long, default_value_t = 4)]
    depth: usize,

    /// Seconds between background `git fetch` cycles. Set to 0 to disable.
    #[arg(long, default_value_t = 300)]
    fetch_interval: u64,

    /// Disable CI status badge (skips `gh run list` calls).
    #[arg(long, default_value_t = false)]
    no_ci: bool,
}

#[derive(Debug, Clone)]
pub enum FetchMsg {
    CycleStarted,
    Started(PathBuf),
    Done(PathBuf),
    CiUpdate(PathBuf, Option<CiInfo>),
    PrUpdate(PathBuf, Option<usize>),
    CycleFinished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CiState {
    Success,
    Failure,
    Running,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct CiInfo {
    pub state: CiState,
    pub name: String,
    pub url: String,
    pub failed_step: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailSection {
    Status,
    Diff,
}

impl DetailSection {
    pub fn next(self) -> Self {
        match self {
            DetailSection::Status => DetailSection::Diff,
            DetailSection::Diff => DetailSection::Status,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            DetailSection::Status => "status",
            DetailSection::Diff => "diff",
        }
    }
}

pub struct App {
    pub repos: Vec<RepoStatus>,
    pub selected: usize,
    pub show_detail: bool,
    pub bases: Vec<PathBuf>,
    pub cols: usize,
    pub last_refresh: Instant,
    pub focus: DetailSection,
    pub detail_cache_key: Option<PathBuf>,
    pub status_content: String,
    pub diff_content: String,
    pub release_tag: String,
    pub release_time_unix: u64,
    pub readme_content: String,
    pub readme_path: String,
    pub status_scroll: u16,
    pub diff_scroll: u16,
    pub readme_scroll: u16,
    pub show_readme: bool,
    pub readme_rendered: bool,
    pub show_stash: bool,
    pub stash_content: String,
    pub pinned: HashSet<PathBuf>,
    pub hidden: HashSet<PathBuf>,
    pub show_hidden: bool,
    pub palette_open: bool,
    pub palette_query: String,
    pub palette_selected: usize,
    pub filter_open: bool,
    pub filter_query: String,
    pub pending_lazygit: Option<PathBuf>,
    pub pending_shell: Option<PathBuf>,
    pub flash: Option<(String, Instant)>,
    pub last_auto_fetch: Option<Instant>,
    pub is_fetching: bool,
    pub fetching: HashSet<PathBuf>,
    pub ci: std::collections::HashMap<PathBuf, CiInfo>,
    pub prs: std::collections::HashMap<PathBuf, usize>,
    pub sort_ci_first: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmd {
    Pin,
    Hide,
    ToggleShowHidden,
    OpenEditor,
    OpenLazyGit,
    OpenShell,
    OpenGitHub,
    OpenActions,
    ToggleStash,
    ToggleSortCi,
    CopyPath,
    RefreshAll,
    FetchFocused,
    PullFocused,
    ToggleReadme,
    ToggleMarkdownMode,
    ToggleDetail,
    FocusStatus,
    FocusDiff,
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
            cmd: Cmd::OpenShell,
            name: "open shell at repo",
            keys: "s",
            desc: "drop into $SHELL in the repo (suspends den)",
        },
        CmdInfo {
            cmd: Cmd::OpenGitHub,
            name: "open on GitHub",
            keys: "g",
            desc: "open the repo's origin URL in browser",
        },
        CmdInfo {
            cmd: Cmd::OpenActions,
            name: "open actions",
            keys: "A",
            desc: "open the GitHub Actions tab in browser",
        },
        CmdInfo {
            cmd: Cmd::ToggleStash,
            name: "stash list",
            keys: "S",
            desc: "show stash entries for the focused repo",
        },
        CmdInfo {
            cmd: Cmd::ToggleSortCi,
            name: "sort: ci red first",
            keys: "O",
            desc: "toggle a leading section that surfaces CI failures",
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
            cmd: Cmd::FetchFocused,
            name: "fetch focused",
            keys: "F",
            desc: "git fetch the focused repo right now",
        },
        CmdInfo {
            cmd: Cmd::PullFocused,
            name: "pull focused",
            keys: "P",
            desc: "git pull --ff-only on the focused repo",
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
        }
    }
    pub fn flash_msg(&mut self, s: impl Into<String>) {
        self.flash = Some((s.into(), Instant::now()));
    }
}

pub fn display_order(app: &App) -> Vec<usize> {
    let q = app.filter_query.trim().to_lowercase();
    let mut idx: Vec<usize> = (0..app.repos.len())
        .filter(|i| {
            let r = &app.repos[*i];
            if !app.show_hidden && app.hidden.contains(&r.path) {
                return false;
            }
            if !q.is_empty() && !r.name.to_lowercase().contains(&q) {
                return false;
            }
            true
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
    let mut bases: Vec<PathBuf> = Vec::with_capacity(args.paths.len());
    for p in &args.paths {
        bases.push(p.canonicalize()?);
    }

    step_start(&format!(
        "scanning {} (depth {})…",
        bases_label(&bases),
        args.depth
    ));
    let mut repo_paths: Vec<PathBuf> = Vec::new();
    for b in &bases {
        for r in discover(b, args.depth) {
            if !repo_paths.contains(&r) {
                repo_paths.push(r);
            }
        }
    }
    if repo_paths.is_empty() {
        step_fail(&format!("no git repos found within depth {}", args.depth));
        return Ok(());
    }
    step_done(&format!(
        "found {} repos in {}",
        repo_paths.len(),
        bases_label(&bases)
    ));

    let total = repo_paths.len();
    let mut statuses: Vec<RepoStatus> = Vec::with_capacity(total);
    for (i, p) in repo_paths.iter().enumerate() {
        let name = p
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("?");
        step_progress(i, total, name);
        statuses.push(status_for(p));
    }
    statuses.sort_by(|a, b| a.name.cmp(&b.name));
    step_done(&format!("read status for {} repos", total));

    if args.no_ci {
        step_warn("CI badges disabled (--no-ci)");
    } else if gh_authed() {
        step_done("gh authenticated");
    } else {
        step_warn(
            "gh: not authenticated — CI badges disabled. Run `gh auth login` or pass --no-ci",
        );
    }

    let pinned = load_pin_set("pins.txt");
    let hidden = load_pin_set("hidden.txt");

    let (tx, rx) = mpsc::channel::<DebounceEventResult>();
    let mut debouncer = new_debouncer(Duration::from_millis(400), move |res| {
        let _ = tx.send(res);
    })?;
    for p in &repo_paths {
        let _ = debouncer.watcher().watch(p, RecursiveMode::Recursive);
    }

    let (fetch_tx, fetch_rx) = mpsc::channel::<FetchMsg>();

    if !args.no_ci {
        let paths = repo_paths.clone();
        let tx = fetch_tx.clone();
        std::thread::spawn(move || {
            for p in &paths {
                let _ = tx.send(FetchMsg::Started(p.clone()));
            }
            for p in &paths {
                let ci = detect_ci(p);
                let _ = tx.send(FetchMsg::CiUpdate(p.clone(), ci));
                let prs = detect_prs(p);
                let _ = tx.send(FetchMsg::PrUpdate(p.clone(), prs));
            }
        });
    }

    if args.fetch_interval > 0 {
        let interval = Duration::from_secs(args.fetch_interval);
        let paths = repo_paths.clone();
        let tx = fetch_tx.clone();
        let ci_enabled = !args.no_ci;
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(15));
            loop {
                let _ = tx.send(FetchMsg::CycleStarted);
                for p in &paths {
                    let _ = tx.send(FetchMsg::Started(p.clone()));
                    let _ = Command::new("git")
                        .env("GIT_TERMINAL_PROMPT", "0")
                        .arg("-C")
                        .arg(p)
                        .args(["fetch", "--quiet", "--no-write-fetch-head", "--all"])
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status();
                    let _ = tx.send(FetchMsg::Done(p.clone()));
                    if ci_enabled {
                        let ci = detect_ci(p);
                        let _ = tx.send(FetchMsg::CiUpdate(p.clone(), ci));
                        let prs = detect_prs(p);
                        let _ = tx.send(FetchMsg::PrUpdate(p.clone(), prs));
                    }
                }
                let _ = tx.send(FetchMsg::CycleFinished);
                std::thread::sleep(interval);
            }
        });
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
        bases: bases.clone(),
        cols: 1,
        last_refresh: Instant::now(),
        focus: DetailSection::Status,
        detail_cache_key: None,
        status_content: String::new(),
        diff_content: String::new(),
        release_tag: String::new(),
        release_time_unix: 0,
        readme_content: String::new(),
        readme_path: String::new(),
        status_scroll: 0,
        diff_scroll: 0,
        readme_scroll: 0,
        show_readme: false,
        readme_rendered: true,
        show_stash: false,
        stash_content: String::new(),
        pinned,
        hidden,
        show_hidden: false,
        palette_open: false,
        palette_query: String::new(),
        palette_selected: 0,
        filter_open: false,
        filter_query: String::new(),
        pending_lazygit: None,
        pending_shell: None,
        flash: None,
        last_auto_fetch: None,
        is_fetching: false,
        fetching: HashSet::new(),
        ci: std::collections::HashMap::new(),
        prs: std::collections::HashMap::new(),
        sort_ci_first: false,
    };

    let res = run(&mut terminal, &mut app, &rx, &fetch_rx, fetch_tx.clone());

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    res
}

fn run<B: ratatui::backend::Backend + std::io::Write>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    rx: &mpsc::Receiver<DebounceEventResult>,
    fetch_rx: &mpsc::Receiver<FetchMsg>,
    fetch_tx: mpsc::Sender<FetchMsg>,
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
                if app.filter_open {
                    match k.code {
                        KeyCode::Char('c') if ctrl => return Ok(()),
                        KeyCode::Esc => {
                            app.filter_open = false;
                            app.filter_query.clear();
                            reselect_into_order(app);
                        }
                        KeyCode::Enter => {
                            app.filter_open = false;
                            reselect_into_order(app);
                        }
                        KeyCode::Backspace => {
                            app.filter_query.pop();
                            reselect_into_order(app);
                        }
                        KeyCode::Char(c) => {
                            app.filter_query.push(c);
                            reselect_into_order(app);
                        }
                        _ => {}
                    }
                    continue;
                }
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
                                if let Some(quit) = execute_cmd(app, c, &fetch_tx) {
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
                    KeyCode::Char('/') => {
                        app.filter_open = true;
                    }
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Char('c') if ctrl => return Ok(()),
                    KeyCode::Char('r') => {
                        refresh_all(app);
                        app.detail_cache_key = None;
                    }
                    KeyCode::Char('F') => fetch_focused(app, &fetch_tx),
                    KeyCode::Char('P') => pull_focused(app, &fetch_tx),
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
                    KeyCode::Char('i') => {
                        app.show_detail = true;
                        app.show_stash = false;
                        app.show_readme = !app.show_readme;
                    }
                    KeyCode::Char('S') => {
                        app.show_detail = true;
                        app.show_readme = false;
                        app.show_stash = !app.show_stash;
                    }
                    KeyCode::Char('m') => {
                        if app.show_readme {
                            app.readme_rendered = !app.readme_rendered;
                            app.readme_scroll = 0;
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
                    KeyCode::Char('s') => action_shell(app),
                    KeyCode::Char('g') => action_github(app),
                    KeyCode::Char('A') => action_actions(app),
                    KeyCode::Char('O') => {
                        app.sort_ci_first = !app.sort_ci_first;
                        app.flash_msg(if app.sort_ci_first {
                            "sort: ci red first"
                        } else {
                            "sort: default"
                        });
                    }
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
                refresh_one_with_flash(app, idx, p);
            }
        }
        if let Some(sel) = &selected_path {
            if changed.iter().any(|p| p == sel) {
                app.detail_cache_key = None;
            }
        }

        let selected_path_for_fetch = app.repos.get(app.selected).map(|r| r.path.clone());
        while let Ok(msg) = fetch_rx.try_recv() {
            match msg {
                FetchMsg::CycleStarted => {
                    app.is_fetching = true;
                }
                FetchMsg::Started(p) => {
                    app.fetching.insert(p);
                }
                FetchMsg::Done(p) => {
                    app.fetching.remove(&p);
                    if let Some(idx) = app.repos.iter().position(|r| r.path == p) {
                        refresh_one_with_flash(app, idx, &p);
                    }
                    if selected_path_for_fetch.as_ref() == Some(&p) {
                        app.detail_cache_key = None;
                    }
                }
                FetchMsg::CiUpdate(p, info) => {
                    app.fetching.remove(&p);
                    match info {
                        Some(s) => {
                            app.ci.insert(p, s);
                        }
                        None => {
                            app.ci.remove(&p);
                        }
                    }
                }
                FetchMsg::PrUpdate(p, count) => match count {
                    Some(n) if n > 0 => {
                        app.prs.insert(p, n);
                    }
                    _ => {
                        app.prs.remove(&p);
                    }
                },
                FetchMsg::CycleFinished => {
                    app.last_auto_fetch = Some(Instant::now());
                    app.is_fetching = false;
                }
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

        if let Some(path) = app.pending_shell.take() {
            disable_raw_mode()?;
            execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
            let status = Command::new(&shell).current_dir(&path).status();
            execute!(terminal.backend_mut(), EnterAlternateScreen)?;
            enable_raw_mode()?;
            terminal.clear()?;
            match status {
                Ok(_) => app.flash_msg("returned from shell"),
                Err(e) => app.flash_msg(format!("shell failed: {}", e)),
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

fn refresh_one_with_flash(app: &mut App, idx: usize, path: &Path) {
    let old_clean = app.repos[idx].is_clean();
    let old_conflict = app.repos[idx].has_conflict();
    let new_status = status_for(path);
    let new_clean = new_status.is_clean();
    let new_conflict = new_status.has_conflict();
    let name = new_status.name.clone();
    app.repos[idx] = new_status;
    if old_conflict != new_conflict && new_conflict {
        app.flash_msg(format!("{} has conflicts", name));
    } else if old_clean != new_clean {
        if new_clean {
            app.flash_msg(format!("{} is clean", name));
        } else {
            app.flash_msg(format!("{} went dirty", name));
        }
    }
}

fn reselect_into_order(app: &mut App) {
    let order = display_order(app);
    if order.is_empty() {
        return;
    }
    if !order.contains(&app.selected) {
        app.selected = order[0];
        app.detail_cache_key = None;
    }
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
    app.stash_content = fetch_stash(&path);
    let (tag, time_unix) = fetch_release(&path);
    app.release_tag = tag;
    app.release_time_unix = time_unix;
    let (readme_path, readme_content) = fetch_readme(&path);
    app.readme_path = readme_path;
    app.readme_content = readme_content;
    app.status_scroll = 0;
    app.diff_scroll = 0;
    app.readme_scroll = 0;
    app.detail_cache_key = Some(path);
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

fn fetch_stash(path: &Path) -> String {
    run_git(
        path,
        &[
            "stash",
            "list",
            "--pretty=format:%gd\x1f%cr\x1f%s",
        ],
    )
}

fn fetch_diff(path: &Path) -> String {
    let s = run_git(path, &["diff", "HEAD", "--no-color", "--patch"]);
    if s.trim().is_empty() {
        run_git(path, &["diff", "--no-color", "--cached", "--patch"])
    } else {
        s
    }
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

fn fetch_release(path: &Path) -> (String, u64) {
    let raw = run_git(
        path,
        &[
            "for-each-ref",
            "--sort=-creatordate",
            "--format=%(refname:short)\x1f%(creatordate:unix)",
            "refs/tags",
            "--count=1",
        ],
    );
    if raw.trim().is_empty() || raw.starts_with("git ") {
        return (String::new(), 0);
    }
    let line = raw.lines().next().unwrap_or("");
    let mut parts = line.splitn(2, '\x1f');
    let name = parts.next().unwrap_or("").trim().to_string();
    if name.is_empty() {
        return (String::new(), 0);
    }
    let time_unix: u64 = parts
        .next()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    (name, time_unix)
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

fn action_shell(app: &mut App) {
    if let Some(repo) = app.repos.get(app.selected) {
        app.pending_shell = Some(repo.path.clone());
    }
}

fn bases_label(bases: &[PathBuf]) -> String {
    match bases.len() {
        0 => String::from("."),
        1 => bases[0].display().to_string(),
        _ => format!("{} +{} more", bases[0].display(), bases.len() - 1),
    }
}

fn step_start(msg: &str) {
    eprint!("\r\x1b[K\x1b[33m⠋\x1b[0m {}", msg);
    let _ = io::stderr().flush();
}

fn step_progress(i: usize, total: usize, name: &str) {
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let frame = FRAMES[i % FRAMES.len()];
    eprint!(
        "\r\x1b[K\x1b[33m{}\x1b[0m reading status ({}/{}) {}",
        frame,
        i + 1,
        total,
        name
    );
    let _ = io::stderr().flush();
}

fn step_done(msg: &str) {
    eprintln!("\r\x1b[K\x1b[32m✓\x1b[0m {}", msg);
}

fn step_warn(msg: &str) {
    eprintln!("\r\x1b[K\x1b[33m⚠\x1b[0m {}", msg);
}

fn step_fail(msg: &str) {
    eprintln!("\r\x1b[K\x1b[31m✗\x1b[0m {}", msg);
}

fn gh_authed() -> bool {
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

fn detect_ci(path: &Path) -> Option<CiInfo> {
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

fn run_id_from_url(url: &str) -> Option<String> {
    url.rsplit('/').next().map(|s| s.to_string())
}

fn failed_step(owner_repo: &str, run_id: &str) -> Option<String> {
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

fn ci_state_from(status: &str, conclusion: &str) -> Option<CiState> {
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

fn github_owner_repo(path: &Path) -> Option<String> {
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

fn detect_prs(path: &Path) -> Option<usize> {
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
            "number",
            "-q",
            "length",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    s.parse().ok()
}

fn current_commit(path: &Path) -> Option<String> {
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

fn pull_focused(app: &mut App, fetch_tx: &mpsc::Sender<FetchMsg>) {
    let Some(repo) = app.repos.get(app.selected) else {
        return;
    };
    let path = repo.path.clone();
    let name = repo.name.clone();
    let tx = fetch_tx.clone();
    let _ = fetch_tx.send(FetchMsg::Started(path.clone()));
    std::thread::spawn(move || {
        let _ = Command::new("git")
            .env("GIT_TERMINAL_PROMPT", "0")
            .arg("-C")
            .arg(&path)
            .args(["pull", "--ff-only", "--quiet"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = tx.send(FetchMsg::Done(path.clone()));
        let ci = detect_ci(&path);
        let _ = tx.send(FetchMsg::CiUpdate(path.clone(), ci));
        let prs = detect_prs(&path);
        let _ = tx.send(FetchMsg::PrUpdate(path, prs));
    });
    app.flash_msg(format!("pulling {}…", name));
}

fn fetch_focused(app: &mut App, fetch_tx: &mpsc::Sender<FetchMsg>) {
    let Some(repo) = app.repos.get(app.selected) else {
        return;
    };
    let path = repo.path.clone();
    let name = repo.name.clone();
    let tx = fetch_tx.clone();
    let _ = fetch_tx.send(FetchMsg::Started(path.clone()));
    std::thread::spawn(move || {
        let _ = Command::new("git")
            .env("GIT_TERMINAL_PROMPT", "0")
            .arg("-C")
            .arg(&path)
            .args(["fetch", "--quiet", "--no-write-fetch-head", "--all"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = tx.send(FetchMsg::Done(path.clone()));
        let ci = detect_ci(&path);
        let _ = tx.send(FetchMsg::CiUpdate(path.clone(), ci));
        let prs = detect_prs(&path);
        let _ = tx.send(FetchMsg::PrUpdate(path, prs));
    });
    app.flash_msg(format!("fetching {}…", name));
}

fn action_actions(app: &mut App) {
    let Some(repo) = app.repos.get(app.selected) else {
        return;
    };
    let Some(owner_repo) = github_owner_repo(&repo.path) else {
        app.flash_msg("not a github remote");
        return;
    };
    let url = if let Some(branch) = &repo.branch {
        format!(
            "https://github.com/{}/actions?query=branch%3A{}",
            owner_repo, branch
        )
    } else {
        format!("https://github.com/{}/actions", owner_repo)
    };
    open_url(&url);
    app.flash_msg(format!("opened {}", url));
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

fn execute_cmd(app: &mut App, cmd: Cmd, fetch_tx: &mpsc::Sender<FetchMsg>) -> Option<bool> {
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
        Cmd::OpenShell => action_shell(app),
        Cmd::OpenGitHub => action_github(app),
        Cmd::OpenActions => action_actions(app),
        Cmd::ToggleStash => {
            app.show_detail = true;
            app.show_readme = false;
            app.show_stash = !app.show_stash;
        }
        Cmd::ToggleSortCi => {
            app.sort_ci_first = !app.sort_ci_first;
            app.flash_msg(if app.sort_ci_first {
                "sort: ci red first"
            } else {
                "sort: default"
            });
        }
        Cmd::CopyPath => action_copy_path(app),
        Cmd::RefreshAll => {
            refresh_all(app);
            app.detail_cache_key = None;
            app.flash_msg("refreshed");
        }
        Cmd::FetchFocused => fetch_focused(app, fetch_tx),
        Cmd::PullFocused => pull_focused(app, fetch_tx),
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
            } else {
                app.flash_msg("open readme first");
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
