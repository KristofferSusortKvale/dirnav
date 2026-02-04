//! TUI file explorer built with ratatui + crossterm.
//!
//! **How it works (high level):**
//! 1. We put the terminal in "raw mode" so we get key events instead of line buffering.
//! 2. We run a loop: read input → update app state → draw UI → repeat until quit.
//! 3. Ratatui doesn't own the terminal; we just draw into a buffer and then flush it to stdout.

use std::fs;
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::sync::OnceLock;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

fn syntax_set() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme_set() -> &'static ThemeSet {
    static SET: OnceLock<ThemeSet> = OnceLock::new();
    SET.get_or_init(ThemeSet::load_defaults)
}

/// Convert syntect highlighting color to ratatui Color (avoids syntect-tui version mismatch with ratatui 0.30).
fn syntect_color_to_ratatui(c: syntect::highlighting::Color) -> Option<Color> {
    if c.a == 0 {
        return None;
    }
    Some(Color::Rgb(c.r, c.g, c.b))
}

/// Convert syntect FontStyle to ratatui Modifier.
fn syntect_font_style_to_modifier(f: FontStyle) -> Modifier {
    let mut m = Modifier::empty();
    if f.contains(FontStyle::BOLD) {
        m |= Modifier::BOLD;
    }
    if f.contains(FontStyle::ITALIC) {
        m |= Modifier::ITALIC;
    }
    if f.contains(FontStyle::UNDERLINE) {
        m |= Modifier::UNDERLINED;
    }
    m
}

/// One entry in the current directory (file or directory).
#[derive(Clone)]
struct DirEntry {
    name: String,
    is_dir: bool,
}

/// All state the UI needs to render and react to input.
struct App {
    /// Current directory we're showing.
    cwd: PathBuf,
    /// Entries in `cwd` (directories first, then files). Sorted by name (case-insensitive).
    entries: Vec<DirEntry>,
    /// Index into `entries` that is currently selected. 0 when list is empty.
    selected: usize,
    /// When false, entries whose name starts with '.' are hidden (except "..").
    show_hidden: bool,
    /// When Some, the preview panel is open showing this file's path and cached content.
    preview_path: Option<PathBuf>,
    /// Cached preview as styled lines (metadata + content). Set when preview_path is set.
    preview_content: Option<Vec<Line<'static>>>,
    /// Vertical scroll offset for the preview (number of lines scrolled down).
    preview_scroll: usize,
    /// True when the preview only shows the first part of the file (file exceeded limit).
    preview_truncated: bool,
}

impl App {
    fn new(initial_cwd: PathBuf) -> Self {
        let mut app = App {
            cwd: initial_cwd,
            entries: Vec::new(),
            selected: 0,
            show_hidden: false,
            preview_path: None,
            preview_content: None,
            preview_scroll: 0,
            preview_truncated: false,
        };
        app.refresh_entries();
        app
    }

    /// Re-read the current directory and set `entries`. Resets selection to 0 and clamps if needed.
    fn refresh_entries(&mut self) {
        let mut entries = read_dir_entries(&self.cwd);
        if !self.show_hidden {
            entries.retain(|e| e.name == ".." || !e.name.starts_with('.'));
        }
        self.entries = entries;
        // Clamp selection so we don't point past the end after refresh (e.g. after going up).
        self.selected = self.selected.min(self.entries.len().saturating_sub(1));
    }

    /// Move selection up by one, wrapping to bottom if at top.
    fn selection_up(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        self.selected = self.selected.saturating_sub(1);
        if self.selected == 0 && !self.entries.is_empty() {
            // Optional: wrap to bottom. Alternatively leave at 0.
            // self.selected = self.entries.len() - 1;
        }
    }

    /// Move selection down by one, wrapping to top if at bottom.
    fn selection_down(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        self.selected = (self.selected + 1).min(self.entries.len() - 1);
    }

    /// Enter the selected directory (if it's a dir) or go to parent if selection is "..".
    fn enter_selected(&mut self) {
        let Some(entry) = self.entries.get(self.selected) else {
            return;
        };
        if entry.name == ".." {
            // Go to parent directory.
            if let Some(parent) = self.cwd.parent() {
                self.cwd = parent.to_path_buf();
                self.selected = 0;
                self.refresh_entries();
            }
            return;
        }
        if entry.is_dir {
            let next = self.cwd.join(&entry.name);
            if next.is_dir() {
                self.cwd = next;
                self.selected = 0;
                self.refresh_entries();
            }
            return;
        }
        // File: open preview panel on the right.
        let path = self.cwd.join(&entry.name);
        if path.is_file() {
            let (content, truncated) = load_file_preview(&path);
            self.preview_content = Some(content);
            self.preview_path = Some(path);
            self.preview_scroll = 0;
            self.preview_truncated = truncated;
        }
    }

    /// Close the preview panel if open.
    fn close_preview(&mut self) {
        self.preview_path = None;
        self.preview_content = None;
        self.preview_scroll = 0;
        self.preview_truncated = false;
    }

    /// Scroll preview down (j). No-op if preview closed.
    fn preview_scroll_down(&mut self) {
        if self.preview_content.is_some() {
            self.preview_scroll = self.preview_scroll.saturating_add(1);
        }
    }

    /// Scroll preview up (k). No-op if preview closed.
    fn preview_scroll_up(&mut self) {
        if self.preview_content.is_some() {
            self.preview_scroll = self.preview_scroll.saturating_sub(1);
        }
    }
}

/// Read directory entries for the given path. Returns dirs first (with ".." at top), then files, sorted by name.
fn read_dir_entries(path: &std::path::Path) -> Vec<DirEntry> {
    let read = match fs::read_dir(path) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    let mut dirs = Vec::new();
    let mut files = Vec::new();

    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
        let de = DirEntry { name, is_dir };
        if de.is_dir {
            dirs.push(de);
        } else {
            files.push(de);
        }
    }

    // Case-insensitive sort so "Apple" comes before "banana".
    dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    let mut out = Vec::new();
    // Only add ".." if we're not at root (so we can go up).
    if path.parent().is_some() {
        out.push(DirEntry {
            name: "..".to_string(),
            is_dir: true,
        });
    }
    out.extend(dirs);
    out.extend(files);
    out
}

/// Build a plain Line from a string (single-style).
fn plain_line(s: impl Into<String>) -> Line<'static> {
    Line::from(Span::raw(s.into()))
}

/// Load a short preview of a file: content only, with syntax highlighting when available.
/// Returns (lines, truncated) where truncated is true if the file was larger than the limit.
fn load_file_preview(path: &std::path::Path) -> (Vec<Line<'static>>, bool) {
    let mut out: Vec<Line<'static>> = Vec::new();

    const MAX_PREVIEW_BYTES: usize = 512 * 1024;
    let content = match fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            out.push(plain_line(format!("Error reading: {}", e)));
            return (out, false);
        }
    };
    if content.is_empty() {
        out.push(plain_line("(empty file)"));
        return (out, false);
    }
    let truncated = content.len() > MAX_PREVIEW_BYTES;
    let text = if truncated {
        &content[..MAX_PREVIEW_BYTES]
    } else {
        content.as_slice()
    };
    let non_print = text
        .iter()
        .filter(|&&b| !b.is_ascii_graphic() && !matches!(b, b' ' | b'\n' | b'\r' | b'\t'))
        .count();
    if non_print > text.len() / 4 {
        out.push(plain_line("(binary file)"));
        return (out, false);
    }
    let content_str = match String::from_utf8(text.to_vec()) {
        Ok(s) => s,
        Err(_) => {
            out.push(plain_line("(not valid UTF-8)"));
            return (out, false);
        }
    };

    let ps = syntax_set();
    let ts = theme_set();
    let syntax = path
        .extension()
        .and_then(|e| e.to_str())
        .and_then(|ext| ps.find_syntax_by_token(ext))
        .unwrap_or_else(|| ps.find_syntax_plain_text());
    let theme = ts
        .themes
        .get("base16-eighties.dark")
        .or_else(|| ts.themes.values().next())
        .expect("theme set has at least one theme");
    let mut highlighter = HighlightLines::new(syntax, theme);

    for line_with_ending in LinesWithEndings::from(&content_str) {
        let line_spans: Vec<Span> = match highlighter.highlight_line(line_with_ending, ps) {
            Ok(segments) => segments
                .into_iter()
                .map(|(syntect_style, text)| {
                    let mut style = Style::default();
                    if let Some(c) = syntect_color_to_ratatui(syntect_style.foreground) {
                        style = style.fg(c);
                    }
                    let mods = syntect_font_style_to_modifier(syntect_style.font_style);
                    if !mods.is_empty() {
                        style = style.add_modifier(mods);
                    }
                    Span::styled(text.to_string(), style)
                })
                .collect(),
            Err(_) => vec![Span::raw(line_with_ending.to_string())],
        };
        out.push(Line::from(line_spans));
    }

    (out, truncated)
}

/// Draw the full UI into the given frame. This is called every frame after handling input.
fn ui(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // Vertical layout: [path bar] [list] [hints]
    let chunks = Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(area);

    // ---- Path bar ----
    let path_text = app.cwd.to_string_lossy();
    let path_title = if app.show_hidden {
        " Path • hidden "
    } else {
        " Path "
    };
    let path_para = Paragraph::new(path_text.as_ref())
        .block(Block::default().borders(Borders::ALL).title(path_title))
        .style(Style::default().fg(Color::Cyan))
        .wrap(Wrap { trim: true });
    frame.render_widget(path_para, chunks[0]);

    // ---- Middle: list only, or list | preview ----
    let (list_chunk, preview_chunk) = if app.preview_path.is_some() {
        let horz = Layout::default()
            .direction(ratatui::layout::Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[1]);
        (horz[0], Some(horz[1]))
    } else {
        (chunks[1], None)
    };

    let items: Vec<ListItem> = app
        .entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let prefix = if e.is_dir { "📁 " } else { "   " };
            let style = if i == app.selected {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(Span::styled(
                format!("{}{}", prefix, e.name),
                style,
            )))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Entries "),
    );
    frame.render_widget(list, list_chunk);

    if let (Some(rect), Some(ref content)) = (preview_chunk, app.preview_content.as_ref()) {
        let base_title = app
            .preview_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Preview".to_string());
        let title = if app.preview_truncated {
            format!(" {} (first 512 KB) ", base_title)
        } else {
            format!(" {} ", base_title)
        };
        let scroll_max = content.len().saturating_sub(rect.height as usize);
        let scroll = app.preview_scroll.min(scroll_max);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(title);
        let lines: Vec<Line<'static>> = content.to_vec();
        let para = Paragraph::new(Text::from(lines))
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((scroll as u16, 0));
        frame.render_widget(para, rect);
    }

    // ---- Key hints ----
    let hints = Line::from(vec![
        Span::styled(" ↑/↓ ", Style::default().fg(Color::DarkGray)),
        Span::raw("or "),
        Span::styled(" k/j ", Style::default().fg(Color::DarkGray)),
        Span::raw("move  "),
        Span::styled(" Enter/l ", Style::default().fg(Color::DarkGray)),
        Span::raw("open  "),
        Span::styled(" h ", Style::default().fg(Color::DarkGray)),
        Span::raw("up  "),
        Span::styled(" H ", Style::default().fg(Color::DarkGray)),
        Span::raw("toggle hidden  "),
        Span::styled(" Esc ", Style::default().fg(Color::DarkGray)),
        Span::raw("close preview / quit  "),
        Span::styled(" j/k ", Style::default().fg(Color::DarkGray)),
        Span::raw("scroll in preview  "),
        Span::styled(" q ", Style::default().fg(Color::DarkGray)),
        Span::raw("quit"),
    ]);
    let hint_para = Paragraph::new(hints).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Keys "),
    );
    frame.render_widget(hint_para, chunks[2]);
}

fn run_app(terminal: &mut ratatui::Terminal<CrosstermBackend<Stdout>>, mut app: App) -> io::Result<()> {
    loop {
        // Draw current state. Ratatui uses double buffering: we draw to an internal buffer,
        // then on draw() it's swapped to the terminal in one go to avoid flicker.
        terminal.draw(|f| ui(f, &app))?;

        // Block until we get an event. This is why we don't need a "sleep" in the loop —
        // the thread blocks on key press.
        if !event::poll(std::time::Duration::from_millis(100))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        // Only act on key *press*, not repeat (avoid moving 10 steps when you hold arrow).
        if key.kind != KeyEventKind::Press {
            continue;
        }

        match key.code {
            KeyCode::Char('q') => break,
            KeyCode::Esc => {
                if app.preview_path.is_some() {
                    app.close_preview();
                } else {
                    break;
                }
            }
            KeyCode::Up => app.selection_up(),
            KeyCode::Down => app.selection_down(),
            KeyCode::Char('k') => {
                if app.preview_path.is_some() {
                    app.preview_scroll_up();
                } else {
                    app.selection_up();
                }
            }
            KeyCode::Char('j') => {
                if app.preview_path.is_some() {
                    app.preview_scroll_down();
                } else {
                    app.selection_down();
                }
            }
            KeyCode::Enter | KeyCode::Char('l') => app.enter_selected(),
            KeyCode::Char('h') => {
                if let Some(parent) = app.cwd.parent() {
                    app.cwd = parent.to_path_buf();
                    app.selected = 0;
                    app.refresh_entries();
                }
            }
            KeyCode::Char('H') => {
                app.show_hidden = !app.show_hidden;
                app.refresh_entries();
            }
            _ => {}
        }
    }
    Ok(())
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    // Start in current directory (or fallback to "/" on Unix, "C:\" on Windows if needed).
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let app = App::new(cwd);

    // 1) Set up terminal: raw mode + alternate screen.
    //    Raw mode = we get key events instead of line-buffered input.
    //    Alternate screen = we draw on a separate buffer; when we exit, the previous
    //    terminal content is restored (no "leftover" UI).
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    // 2) CrosstermBackend lets ratatui draw using crossterm's terminal API.
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    // 3) Run the app loop. When it returns, restore terminal state.
    let result = run_app(&mut terminal, app);

    // 4) Restore terminal so the shell looks normal again.
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result?;
    Ok(())
}
