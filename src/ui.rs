use crate::brand::{AMBER, AMBER_STRONG, CONFLICT, FOREST, IVORY, STONE, STONE_STRONG};
use crate::repo::RepoStatus;
use crate::{App, DetailSection};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};
use std::time::{Duration, SystemTime};

const TILE_MIN_W: u16 = 32;
const TILE_H: u16 = 7;
const MAX_COLS: usize = 4;
const SECTION_HEADER_H: u16 = 1;
const SECTION_GAP_H: u16 = 1;

#[derive(Copy, Clone, Eq, PartialEq)]
enum SectionKind {
    Conflicts,
    Dirty,
    Clean,
    Hidden,
}

impl SectionKind {
    fn label(self) -> &'static str {
        match self {
            SectionKind::Conflicts => "conflicts",
            SectionKind::Dirty => "dirty",
            SectionKind::Clean => "clean",
            SectionKind::Hidden => "hidden",
        }
    }

    fn color(self) -> ratatui::style::Color {
        match self {
            SectionKind::Conflicts => CONFLICT,
            SectionKind::Dirty => AMBER,
            SectionKind::Clean => FOREST,
            SectionKind::Hidden => STONE_STRONG,
        }
    }
}

fn classify(app: &App, repo: &RepoStatus) -> SectionKind {
    if app.hidden.contains(&repo.path) {
        return SectionKind::Hidden;
    }
    if repo.has_conflict() || repo.error.is_some() {
        return SectionKind::Conflicts;
    }
    if !repo.is_clean() {
        return SectionKind::Dirty;
    }
    SectionKind::Clean
}

fn group_by_section(app: &App, order: &[usize]) -> Vec<(SectionKind, Vec<usize>)> {
    let mut conflicts = Vec::new();
    let mut dirty = Vec::new();
    let mut clean = Vec::new();
    let mut hidden = Vec::new();
    for &i in order {
        let r = &app.repos[i];
        match classify(app, r) {
            SectionKind::Conflicts => conflicts.push(i),
            SectionKind::Dirty => dirty.push(i),
            SectionKind::Clean => clean.push(i),
            SectionKind::Hidden => hidden.push(i),
        }
    }
    [
        (SectionKind::Conflicts, conflicts),
        (SectionKind::Dirty, dirty),
        (SectionKind::Clean, clean),
        (SectionKind::Hidden, hidden),
    ]
    .into_iter()
    .filter(|(_, v)| !v.is_empty())
    .collect()
}

fn render_section_header(f: &mut Frame, area: Rect, kind: SectionKind, count: usize) {
    let label = kind.label();
    let count_str = format!(" {} ", count);
    let prefix_len = 3 + label.len() + count_str.len() + 1;
    let trailing = (area.width as usize).saturating_sub(prefix_len);
    let line = Line::from(vec![
        Span::styled("── ", Style::default().fg(STONE_STRONG)),
        Span::styled(
            label,
            Style::default().fg(kind.color()).add_modifier(Modifier::BOLD),
        ),
        Span::styled(count_str, Style::default().fg(STONE)),
        Span::styled("─".repeat(trailing), Style::default().fg(STONE_STRONG)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

pub fn render(f: &mut Frame, app: &mut App) {
    let size = f.area();

    let main_area = if app.show_detail && !app.repos.is_empty() {
        let halves = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(size);
        render_detail(f, halves[1], app);
        halves[0]
    } else {
        size
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(main_area);

    render_header(f, chunks[0], app);
    render_grid(f, chunks[1], app);
    render_footer(f, chunks[2], app);

    if app.palette_open {
        render_palette(f, app);
    }
}

fn render_palette(f: &mut Frame, app: &App) {
    let size = f.area();
    let w = 78u16.min(size.width.saturating_sub(4));
    let max_h = size.height.saturating_sub(4);
    let h = 22u16.min(max_h.max(8));
    let x = size.width.saturating_sub(w) / 2;
    let y = size.height.saturating_sub(h) / 3;
    let area = ratatui::layout::Rect::new(x, y, w, h);

    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Thick)
        .border_style(Style::default().fg(AMBER_STRONG))
        .title(Line::from(vec![Span::styled(
            " command palette ",
            Style::default()
                .fg(AMBER_STRONG)
                .add_modifier(Modifier::BOLD),
        )]));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(0)])
        .split(inner);

    let query_line = Line::from(vec![
        Span::styled(": ", Style::default().fg(AMBER_STRONG)),
        Span::styled(app.palette_query.clone(), Style::default().fg(IVORY)),
        Span::styled(
            "▌",
            Style::default()
                .fg(AMBER_STRONG)
                .add_modifier(Modifier::SLOW_BLINK),
        ),
    ]);
    f.render_widget(Paragraph::new(query_line), chunks[0]);

    let matches = crate::filter_commands(&app.palette_query);
    if matches.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "no matches",
                Style::default().fg(STONE),
            ))),
            chunks[1],
        );
        return;
    }

    let visible_rows = chunks[1].height as usize;
    let total = matches.len();
    let start = if app.palette_selected >= visible_rows {
        app.palette_selected + 1 - visible_rows
    } else {
        0
    };
    let end = (start + visible_rows).min(total);

    let mut lines: Vec<Line> = Vec::new();
    for (i, c) in matches[start..end].iter().enumerate() {
        let abs_i = start + i;
        let selected = abs_i == app.palette_selected;
        let arrow_style = if selected {
            Style::default()
                .fg(AMBER_STRONG)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(STONE_STRONG)
        };
        let name_style = if selected {
            Style::default().fg(IVORY).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(STONE)
        };
        let key_style = if selected {
            Style::default()
                .fg(AMBER_STRONG)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(STONE_STRONG)
        };
        let desc_style = Style::default().fg(STONE);
        lines.push(Line::from(vec![
            Span::styled(if selected { "▸ " } else { "  " }, arrow_style),
            Span::styled(format!("{:<22}", c.name), name_style),
            Span::styled(format!("{:>4}  ", c.keys), key_style),
            Span::styled(c.desc.to_string(), desc_style),
        ]));
    }
    f.render_widget(Paragraph::new(lines), chunks[1]);
}

fn render_header(f: &mut Frame, area: Rect, app: &App) {
    let n = app.repos.len();
    let dirty = app
        .repos
        .iter()
        .filter(|r| !r.is_clean() && r.error.is_none() && !r.has_conflict())
        .count();
    let conflicts = app.repos.iter().filter(|r| r.has_conflict()).count();
    let pinned = app.pinned.len();
    let hidden = app
        .repos
        .iter()
        .filter(|r| app.hidden.contains(&r.path))
        .count();
    let line = Line::from(vec![
        Span::styled(
            "den",
            Style::default()
                .fg(AMBER_STRONG)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(app.base.display().to_string(), Style::default().fg(AMBER)),
        Span::raw("  "),
        Span::styled(format!("{n} repos"), Style::default().fg(AMBER_STRONG)),
        Span::raw("  "),
        Span::styled(
            format!("{dirty} dirty"),
            Style::default().fg(if dirty > 0 { AMBER_STRONG } else { STONE_STRONG }),
        ),
        Span::raw("  "),
        Span::styled(
            format!("{conflicts} conflicts"),
            Style::default().fg(if conflicts > 0 { CONFLICT } else { STONE_STRONG }),
        ),
        Span::raw("  "),
        Span::styled(
            format!("{pinned} pinned"),
            Style::default().fg(if pinned > 0 { AMBER_STRONG } else { STONE_STRONG }),
        ),
        Span::raw("  "),
        Span::styled(
            format!(
                "{hidden} hidden{}",
                if app.show_hidden { " (shown)" } else { "" }
            ),
            Style::default().fg(if hidden > 0 { AMBER } else { STONE_STRONG }),
        ),
    ]);
    let sync_line = Line::from(vec![sync_span(app)]);
    f.render_widget(Paragraph::new(vec![line, sync_line]), area);
}

fn sync_span(app: &App) -> Span<'static> {
    if app.is_fetching {
        return Span::styled(
            "↻ syncing…".to_string(),
            Style::default().fg(AMBER_STRONG).add_modifier(Modifier::BOLD),
        );
    }
    let label = match app.last_auto_fetch {
        None => "last sync: —".to_string(),
        Some(t) => format!("last sync: {} ago", relative_short(t.elapsed())),
    };
    Span::styled(label, Style::default().fg(STONE_STRONG))
}

fn relative_short(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{} min", secs / 60)
    } else {
        format!("{}h", secs / 3600)
    }
}

fn render_footer(f: &mut Frame, area: Rect, app: &App) {
    if let Some((msg, _)) = &app.flash {
        let line = Line::from(vec![Span::styled(
            msg.clone(),
            Style::default().fg(AMBER_STRONG).add_modifier(Modifier::BOLD),
        )]);
        f.render_widget(Paragraph::new(line), area);
        return;
    }
    let key = |k: &str| Span::styled(k.to_string(), Style::default().fg(AMBER_STRONG));
    let label = |s: &str| Span::styled(format!(" {}  ", s), Style::default().fg(STONE));
    let mut spans = vec![
        key(":"),
        label("commands"),
        key("↑↓←→"),
        label("select"),
        key("↵"),
        label("detail"),
        key("p"),
        label("pin"),
        key("x"),
        label("hide"),
    ];
    if app.show_detail {
        if app.show_readme {
            spans.push(key("i"));
            spans.push(label("close"));
            spans.push(key("m"));
            spans.push(label(if app.readme_rendered {
                "raw"
            } else {
                "rendered"
            }));
        } else {
            spans.push(key("1/2"));
            spans.push(label("focus"));
            spans.push(key("i"));
            spans.push(label("readme"));
        }
        spans.push(key("PgUp/PgDn"));
        spans.push(label("scroll"));
    } else {
        spans.push(key("i"));
        spans.push(label("readme"));
    }
    spans.push(key("r"));
    spans.push(label("refresh"));
    spans.push(key("q"));
    spans.push(label("quit"));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_grid(f: &mut Frame, area: Rect, app: &mut App) {
    if app.repos.is_empty() {
        let p = Paragraph::new("no repos").style(Style::default().fg(STONE));
        f.render_widget(p, area);
        app.cols = 1;
        return;
    }

    let avail_cols = ((area.width / TILE_MIN_W).max(1)) as usize;
    let cols = avail_cols.min(MAX_COLS);
    app.cols = cols;
    let order = crate::display_order(app);
    let groups = group_by_section(app, &order);

    let mut constraints: Vec<Constraint> = Vec::new();
    for (i, (_, indices)) in groups.iter().enumerate() {
        if i > 0 {
            constraints.push(Constraint::Length(SECTION_GAP_H));
        }
        constraints.push(Constraint::Length(SECTION_HEADER_H));
        let n_rows = (indices.len() + cols - 1) / cols;
        for _ in 0..n_rows {
            constraints.push(Constraint::Length(TILE_H));
        }
    }
    constraints.push(Constraint::Min(0));

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let col_constraints: Vec<Constraint> = (0..cols)
        .map(|_| Constraint::Ratio(1, cols as u32))
        .collect();

    let mut chunk_idx = 0;
    for (i, (kind, indices)) in groups.iter().enumerate() {
        if i > 0 {
            chunk_idx += 1;
        }
        if chunk_idx >= chunks.len() {
            break;
        }
        render_section_header(f, chunks[chunk_idx], *kind, indices.len());
        chunk_idx += 1;
        let n_rows = (indices.len() + cols - 1) / cols;
        for r in 0..n_rows {
            if chunk_idx >= chunks.len() {
                break;
            }
            let cells = Layout::default()
                .direction(Direction::Horizontal)
                .constraints(col_constraints.clone())
                .split(chunks[chunk_idx]);
            for c in 0..cols {
                let k = r * cols + c;
                if k >= indices.len() {
                    break;
                }
                let repo_idx = indices[k];
                let repo = &app.repos[repo_idx];
                let pinned = app.pinned.contains(&repo.path);
                let hidden = app.hidden.contains(&repo.path);
                render_tile(
                    f,
                    cells[c],
                    repo,
                    repo_idx == app.selected,
                    pinned,
                    hidden,
                );
            }
            chunk_idx += 1;
        }
    }
}

fn render_tile(
    f: &mut Frame,
    area: Rect,
    repo: &RepoStatus,
    selected: bool,
    pinned: bool,
    is_hidden: bool,
) {
    let state_color = if repo.error.is_some() || repo.has_conflict() {
        CONFLICT
    } else if repo.is_uninitialized() {
        STONE_STRONG
    } else if !repo.is_clean() {
        AMBER
    } else {
        FOREST
    };

    let border_color = if selected {
        IVORY
    } else if is_hidden {
        STONE_STRONG
    } else {
        state_color
    };
    let border_style = if selected {
        Style::default()
            .fg(border_color)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(border_color)
    };

    let mut title_spans: Vec<Span> = Vec::new();
    if is_hidden {
        title_spans.push(Span::styled("✗ ", Style::default().fg(STONE_STRONG)));
    } else if pinned {
        title_spans.push(Span::styled("★ ", Style::default().fg(AMBER_STRONG)));
    } else {
        title_spans.push(Span::raw(" "));
    }
    let name_color = if is_hidden { STONE } else { IVORY };
    title_spans.push(Span::styled(
        format!("{} ", repo.name),
        Style::default().fg(name_color).add_modifier(Modifier::BOLD),
    ));
    let title = Line::from(title_spans);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(if selected {
            BorderType::Thick
        } else {
            BorderType::Plain
        })
        .border_style(border_style)
        .title(title);

    let mut lines: Vec<Line> = Vec::new();

    if let Some(err) = &repo.error {
        lines.push(Line::from(Span::styled(
            err.clone(),
            Style::default().fg(CONFLICT),
        )));
    } else {
        let mut spans: Vec<Span> = Vec::new();
        spans.push(Span::styled(" ", Style::default().fg(AMBER_STRONG)));
        if let Some(b) = &repo.branch {
            spans.push(Span::styled(
                b.clone(),
                Style::default()
                    .fg(AMBER_STRONG)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled("(no head)".to_string(), Style::default().fg(STONE)));
        }
        if repo.ahead > 0 || repo.behind > 0 {
            spans.push(Span::raw("  "));
            if repo.ahead > 0 {
                spans.push(Span::styled(
                    format!("↑{}", repo.ahead),
                    Style::default().fg(FOREST).add_modifier(Modifier::BOLD),
                ));
            }
            if repo.behind > 0 {
                if repo.ahead > 0 {
                    spans.push(Span::raw(" "));
                }
                spans.push(Span::styled(
                    format!("↓{}", repo.behind),
                    Style::default().fg(AMBER),
                ));
            }
        }
        if let Some(tag) = &repo.latest_tag {
            spans.push(Span::raw("  "));
            spans.push(Span::styled("◇ ", Style::default().fg(STONE)));
            spans.push(Span::styled(
                tag.name.clone(),
                Style::default().fg(AMBER_STRONG).add_modifier(Modifier::BOLD),
            ));
            if tag.commits_since > 0 {
                spans.push(Span::styled(
                    format!("+{}", tag.commits_since),
                    Style::default().fg(STONE_STRONG),
                ));
            }
        }
        lines.push(Line::from(spans));

        let mut spans: Vec<Span> = Vec::new();
        if repo.staged > 0 {
            spans.push(Span::styled(
                format!("+{} ", repo.staged),
                Style::default().fg(FOREST).add_modifier(Modifier::BOLD),
            ));
        }
        if repo.modified > 0 {
            spans.push(Span::styled(
                format!("~{} ", repo.modified),
                Style::default().fg(AMBER),
            ));
        }
        if repo.untracked > 0 {
            spans.push(Span::styled(
                format!("?{} ", repo.untracked),
                Style::default().fg(STONE_STRONG),
            ));
        }
        if repo.conflicted > 0 {
            spans.push(Span::styled(
                format!("!{} ", repo.conflicted),
                Style::default().fg(CONFLICT).add_modifier(Modifier::BOLD),
            ));
        }
        if repo.stashed > 0 {
            spans.push(Span::styled(
                format!("⌂{} ", repo.stashed),
                Style::default().fg(AMBER_STRONG),
            ));
        }
        if spans.is_empty() {
            spans.push(Span::styled(
                "clean".to_string(),
                Style::default().fg(STONE),
            ));
        }
        lines.push(Line::from(spans));

        if let Some(c) = &repo.last_commit {
            let rel = relative_time(c.time);
            let body_w = area.width.saturating_sub(2) as usize;
            let reserved = c.short_sha.len() + rel.len() + 3;
            let summary_max = body_w.saturating_sub(reserved);
            let summary: String = c.summary.chars().take(summary_max).collect();
            lines.push(Line::from(vec![
                Span::styled(c.short_sha.clone(), Style::default().fg(STONE)),
                Span::raw(" "),
                Span::styled(summary, Style::default().fg(IVORY)),
                Span::raw(" "),
                Span::styled(rel, Style::default().fg(STONE)),
            ]));
        }
    }

    let para = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    f.render_widget(para, area);
}

fn render_detail(f: &mut Frame, area: Rect, app: &App) {
    let repo = &app.repos[app.selected];
    let pinned = app.pinned.contains(&repo.path);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(STONE))
        .title(detail_title(repo, pinned, app));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.show_readme {
        render_readme(f, inner, app);
        return;
    }

    let status_lines = parse_status(&app.status_content);
    let status_h = (status_lines.len() as u16 + 2).clamp(3, 10);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(status_h), Constraint::Min(4)])
        .split(inner);

    render_section(
        f,
        chunks[0],
        app,
        DetailSection::Status,
        status_lines,
        app.status_scroll,
    );
    render_section(
        f,
        chunks[1],
        app,
        DetailSection::Diff,
        parse_diff(&app.diff_content),
        app.diff_scroll,
    );
}

fn render_readme(f: &mut Frame, area: Rect, app: &App) {
    let mode_label = if app.readme_rendered { "rendered" } else { "raw" };
    let header_text = if app.readme_path.is_empty() {
        format!("readme · {}", mode_label)
    } else {
        format!("readme · {} · {}", mode_label, app.readme_path)
    };
    let header = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(AMBER_STRONG))
        .title(Line::from(vec![Span::styled(
            format!(" {} ", header_text),
            Style::default()
                .fg(AMBER_STRONG)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )]));
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(0)])
        .split(area);
    f.render_widget(header, chunks[0]);

    let lines = if app.readme_content.is_empty() {
        vec![Line::from(Span::styled(
            "no README found",
            Style::default().fg(STONE),
        ))]
    } else if app.readme_rendered {
        crate::markdown::render(&app.readme_content)
    } else {
        parse_markdown_raw(&app.readme_content)
    };
    let para = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((app.readme_scroll, 0));
    f.render_widget(para, chunks[1]);
}

fn parse_markdown_raw(s: &str) -> Vec<Line<'static>> {
    s.lines()
        .map(|l| Line::from(Span::styled(l.to_string(), Style::default().fg(IVORY))))
        .collect()
}

fn detail_title(repo: &RepoStatus, pinned: bool, app: &App) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::raw(" "));
    if pinned {
        spans.push(Span::styled("★ ", Style::default().fg(AMBER_STRONG)));
    }
    spans.push(Span::styled(
        format!("{} ", repo.name),
        Style::default().fg(IVORY).add_modifier(Modifier::BOLD),
    ));
    if let Some(b) = &repo.branch {
        spans.push(Span::styled(
            format!(" {} ", b),
            Style::default()
                .fg(AMBER_STRONG)
                .add_modifier(Modifier::BOLD),
        ));
    }
    if repo.ahead > 0 {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            format!("↑{}", repo.ahead),
            Style::default().fg(FOREST).add_modifier(Modifier::BOLD),
        ));
    }
    if repo.behind > 0 {
        spans.push(Span::raw(if repo.ahead > 0 { " " } else { "  " }));
        spans.push(Span::styled(
            format!("↓{}", repo.behind),
            Style::default().fg(AMBER),
        ));
    }
    if !app.release_tag.is_empty() {
        let time = SystemTime::UNIX_EPOCH + Duration::from_secs(app.release_time_unix);
        spans.push(Span::raw("  ·  "));
        spans.push(Span::styled(
            format!("{} ", app.release_tag.clone()),
            Style::default().fg(STONE),
        ));
        spans.push(Span::styled(
            format!("{} ago", relative_time(time)),
            Style::default().fg(STONE_STRONG),
        ));
    }
    Line::from(spans)
}

fn render_section(
    f: &mut Frame,
    area: Rect,
    app: &App,
    section: DetailSection,
    lines: Vec<Line>,
    scroll: u16,
) {
    let focused = app.focus == section;
    let title_style = if focused {
        Style::default()
            .fg(AMBER_STRONG)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    } else {
        Style::default()
            .fg(STONE)
            .add_modifier(Modifier::BOLD)
    };
    let num = match section {
        DetailSection::Status => "1",
        DetailSection::Diff => "2",
    };
    let title = Line::from(vec![Span::styled(
        format!(" {} {} ", num, section.label()),
        title_style,
    )]);
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(if focused { AMBER_STRONG } else { STONE }))
        .title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let para = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    f.render_widget(para, inner);
}

fn parse_status(s: &str) -> Vec<Line<'_>> {
    let mut out: Vec<Line> = Vec::new();
    if s.trim().is_empty() {
        out.push(Line::from(Span::styled(
            "working tree clean",
            Style::default().fg(FOREST),
        )));
        return out;
    }
    for l in s.lines() {
        if l.starts_with("## ") {
            continue;
        }
        let prefix: String = l.chars().take(2).collect();
        let rest: String = l.chars().skip(3).collect();
        let color = match prefix.trim() {
            "??" => STONE_STRONG,
            "M" | "MM" | "AM" => AMBER,
            "A" => FOREST,
            "D" | "AD" => CONFLICT,
            "R" | "RM" => AMBER,
            "UU" | "AA" | "DD" => CONFLICT,
            _ => STONE,
        };
        out.push(Line::from(vec![
            Span::styled(format!("{:>2} ", prefix), Style::default().fg(color)),
            Span::styled(rest, Style::default().fg(IVORY)),
        ]));
    }
    out
}

fn parse_diff(s: &str) -> Vec<Line<'_>> {
    if s.trim().is_empty() {
        return vec![Line::from(Span::styled(
            "no changes vs HEAD",
            Style::default().fg(FOREST),
        ))];
    }
    let mut out: Vec<Line> = Vec::new();
    for l in s.lines() {
        let style = if l.starts_with("diff --git") || l.starts_with("index ") {
            Style::default()
                .fg(AMBER_STRONG)
                .add_modifier(Modifier::BOLD)
        } else if l.starts_with("+++") || l.starts_with("---") {
            Style::default().fg(STONE)
        } else if l.starts_with("@@") {
            Style::default().fg(AMBER)
        } else if l.starts_with('+') {
            Style::default().fg(FOREST)
        } else if l.starts_with('-') {
            Style::default().fg(CONFLICT)
        } else {
            Style::default().fg(STONE)
        };
        out.push(Line::from(Span::styled(l.to_string(), style)));
    }
    out
}

fn relative_time(t: SystemTime) -> String {
    let now = SystemTime::now();
    let secs = now.duration_since(t).map(|d| d.as_secs()).unwrap_or(0);
    if secs < 60 {
        return format!("{}s", secs);
    }
    let mins = secs / 60;
    if mins < 60 {
        return format!("{}m", mins);
    }
    let hrs = mins / 60;
    if hrs < 24 {
        return format!("{}h", hrs);
    }
    let days = hrs / 24;
    if days < 30 {
        return format!("{}d", days);
    }
    let months = days / 30;
    if months < 12 {
        return format!("{}mo", months);
    }
    let years = days / 365;
    format!("{}y", years)
}
