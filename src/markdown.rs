use crate::brand::{AMBER, AMBER_STRONG, IVORY, STONE, STONE_STRONG};
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

pub fn render(src: &str) -> Vec<Line<'static>> {
    let mut r = Renderer::new();
    for event in Parser::new(src) {
        r.handle(event);
    }
    r.flush_line();
    r.out
}

struct Renderer {
    out: Vec<Line<'static>>,
    current: Vec<Span<'static>>,
    style_stack: Vec<Style>,
    list_stack: Vec<Option<u64>>,
    indent: u16,
    in_code_block: bool,
    needs_blank: bool,
    pending_link: Option<String>,
}

impl Renderer {
    fn new() -> Self {
        Self {
            out: Vec::new(),
            current: Vec::new(),
            style_stack: vec![Style::default().fg(IVORY)],
            list_stack: Vec::new(),
            indent: 0,
            in_code_block: false,
            needs_blank: false,
            pending_link: None,
        }
    }

    fn cur_style(&self) -> Style {
        *self.style_stack.last().unwrap()
    }

    fn flush_line(&mut self) {
        if !self.current.is_empty() {
            let spans = std::mem::take(&mut self.current);
            self.out.push(Line::from(spans));
        }
    }

    fn end_block(&mut self) {
        self.flush_line();
        self.needs_blank = true;
    }

    fn ensure_blank(&mut self) {
        if self.needs_blank {
            self.out.push(Line::raw(""));
            self.needs_blank = false;
        }
    }

    fn push_indent(&mut self) {
        if self.indent > 0 && self.current.is_empty() {
            self.current.push(Span::raw("  ".repeat(self.indent as usize)));
        }
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => self.text(t.into_string()),
            Event::Code(t) => {
                let style = Style::default()
                    .fg(AMBER_STRONG)
                    .add_modifier(Modifier::ITALIC);
                self.current.push(Span::styled(t.into_string(), style));
            }
            Event::SoftBreak | Event::HardBreak => {
                if self.in_code_block {
                    self.flush_line();
                } else {
                    self.current.push(Span::raw(" "));
                }
            }
            Event::Rule => {
                self.ensure_blank();
                self.out.push(Line::from(Span::styled(
                    "─".repeat(40),
                    Style::default().fg(STONE),
                )));
                self.needs_blank = true;
            }
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Heading { level, .. } => {
                self.ensure_blank();
                let style = match level {
                    HeadingLevel::H1 => Style::default()
                        .fg(AMBER_STRONG)
                        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                    HeadingLevel::H2 => Style::default()
                        .fg(AMBER_STRONG)
                        .add_modifier(Modifier::BOLD),
                    HeadingLevel::H3 => Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
                    _ => Style::default().fg(AMBER),
                };
                self.style_stack.push(style);
            }
            Tag::Paragraph => {
                self.ensure_blank();
                self.push_indent();
            }
            Tag::BlockQuote(_) => {
                self.ensure_blank();
                self.style_stack.push(Style::default().fg(STONE));
            }
            Tag::CodeBlock(_kind) => {
                self.in_code_block = true;
                self.ensure_blank();
                self.style_stack.push(Style::default().fg(STONE_STRONG));
            }
            Tag::List(start) => {
                self.list_stack.push(start);
                self.indent += 1;
                if self.indent == 1 {
                    self.ensure_blank();
                }
            }
            Tag::Item => {
                let pad = "  ".repeat(self.indent.saturating_sub(1) as usize);
                let bullet = match self.list_stack.last_mut() {
                    Some(Some(n)) => {
                        let s = format!("{}. ", n);
                        *n += 1;
                        s
                    }
                    _ => "• ".to_string(),
                };
                self.current.push(Span::raw(pad));
                self.current
                    .push(Span::styled(bullet, Style::default().fg(AMBER_STRONG)));
            }
            Tag::Strong => {
                let s = self.cur_style().add_modifier(Modifier::BOLD);
                self.style_stack.push(s);
            }
            Tag::Emphasis => {
                let s = self.cur_style().add_modifier(Modifier::ITALIC);
                self.style_stack.push(s);
            }
            Tag::Strikethrough => {
                let s = self.cur_style().add_modifier(Modifier::CROSSED_OUT);
                self.style_stack.push(s);
            }
            Tag::Link { dest_url, .. } => {
                let s = self
                    .cur_style()
                    .fg(AMBER_STRONG)
                    .add_modifier(Modifier::UNDERLINED);
                self.style_stack.push(s);
                self.pending_link = Some(dest_url.into_string());
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Heading(_) => {
                self.style_stack.pop();
                self.end_block();
            }
            TagEnd::Paragraph => {
                self.end_block();
            }
            TagEnd::BlockQuote(_) => {
                self.style_stack.pop();
                self.needs_blank = true;
            }
            TagEnd::CodeBlock => {
                self.in_code_block = false;
                self.style_stack.pop();
                self.flush_line();
                self.needs_blank = true;
            }
            TagEnd::List(_) => {
                self.list_stack.pop();
                self.indent = self.indent.saturating_sub(1);
                if self.indent == 0 {
                    self.needs_blank = true;
                }
            }
            TagEnd::Item => {
                self.flush_line();
            }
            TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough => {
                self.style_stack.pop();
            }
            TagEnd::Link => {
                self.style_stack.pop();
                if let Some(url) = self.pending_link.take() {
                    self.current.push(Span::styled(
                        format!(" ({})", url),
                        Style::default().fg(STONE),
                    ));
                }
            }
            _ => {}
        }
    }

    fn text(&mut self, t: String) {
        let style = self.cur_style();
        if self.in_code_block {
            for (i, line) in t.split('\n').enumerate() {
                if i > 0 {
                    self.flush_line();
                }
                self.current
                    .push(Span::styled("    ".to_string(), Style::default()));
                self.current.push(Span::styled(line.to_string(), style));
            }
        } else {
            self.current.push(Span::styled(t, style));
        }
    }
}
