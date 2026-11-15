//! Full-screen interactive reader (ratatui + crossterm).
//!
//! Keys (Vim-flavoured):
//!   j/k, Ctrl-d/Ctrl-u, g/G  scroll        J/K      next/previous paragraph
//!   h/l, Left/Right          move token   v        visual selection
//!   1-6                     apply colour  0/x      remove highlight
//!   n                        edit note     b        sidebar    tab (in sidebar)
//!   / n N, Ctrl-n            search        f        references drawer
//!   Enter                    follow link   H/L      history
//!   r                        refresh       q        quit

pub mod notes;
pub mod reader;
pub mod sidebar;

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line as TuiLine, Span as TuiSpan};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use ratatui::Terminal;

use wol_core::dom::parse_document;

use crate::context::Context;
use crate::render::article::RenderOptions;
use crate::render::{ColorDepth, Glyphs, Theme};
use crate::Cli;

use self::notes::{EditorKey, EditorOutcome, NoteEditor};
use self::reader::Reader;
use self::sidebar::Sidebar;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    Reading,
    Editing(NoteEditor),
    Searching(String),
    Palette,
    References,
}

struct App {
    context: Context,
    theme: Theme,
    reader: Reader,
    sidebar: Sidebar,
    mode: Mode,
    history: Vec<String>,
    history_index: usize,
    current: String,
    message: Option<String>,
    pending_g: bool,
    focus_sidebar: bool,
}

pub async fn run(
    cli: &Cli,
    context: &Context,
    target: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let target = target
        .map(str::to_string)
        .or_else(|| latest_cached(&context))
        .ok_or("nothing to open: pass a URL/ref (e.g. `wol tui /en/wol/d/r1/lp-e/2025441`)")?;

    let theme = Theme::new(ColorDepth::detect(cli.no_color), Glyphs::detect(cli.ascii));
    let (document, current) = load(&context, &target, cli.offline, false).await?;
    let marks = context
        .state
        .get_stored_index()
        .ok()
        .flatten()
        .unwrap_or_default();
    let options = RenderOptions {
        width: 100,
        gutter: true,
        show_notes: true,
        show_footnotes: true,
    };
    let reader = Reader::new(document, marks, &theme, options);

    let mut app = App {
        context: context.clone(),
        theme,
        reader,
        sidebar: Sidebar::default(),
        mode: Mode::Reading,
        history: vec![current.clone()],
        history_index: 0,
        current,
        message: None,
        pending_g: false,
        focus_sidebar: false,
    };
    app.resize(crate::width_for(cli));

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = event_loop(&mut terminal, &mut app).await;
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result.map_err(|err| err.into())
}

fn latest_cached(context: &Context) -> Option<String> {
    context
        .cache
        .list_entries(1, 0, Some("/wol/d/"))
        .ok()
        .and_then(|entries| entries.into_iter().next())
        .map(|entry| entry.url.as_str().to_string())
}

async fn load(
    context: &Context,
    target: &str,
    offline: bool,
    refresh: bool,
) -> Result<(wol_core::dom::Document, String), Box<dyn std::error::Error>> {
    let page = if refresh {
        let canonical = context.canonicalize(target)?;
        context.fetch(canonical).await?
    } else {
        context.load(target, offline).await?
    };
    let mut document = parse_document(&page.html);
    document.source_url = Some(page.canonical.as_str().to_string());
    Ok((document, page.canonical.as_str().to_string()))
}

impl App {
    fn resize(&mut self, width: usize) {
        let sidebar_width = if self.sidebar.visible { 30 } else { 0 };
        let body = width.saturating_sub(sidebar_width + 4).max(20);
        self.reader.rebuild(
            &self.theme,
            RenderOptions {
                width: body,
                gutter: true,
                show_notes: true,
                show_footnotes: true,
            },
        );
    }

    fn set_message(&mut self, text: impl Into<String>) {
        self.message = Some(text.into());
    }

    fn save_marks(&mut self) {
        if let Err(err) = self.context.state.set_stored_index(&self.reader.marks) {
            self.set_message(format!("could not save marks: {err}"));
        }
    }
}

async fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> io::Result<()> {
    loop {
        terminal.draw(|frame| draw(frame, app))?;
        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind == event::KeyEventKind::Release {
            continue;
        }
        if !handle_key(app, key).await? {
            return Ok(());
        }
    }
}

async fn handle_key(app: &mut App, key: KeyEvent) -> io::Result<bool> {
    // Modal handling first.
    match app.mode.clone() {
        Mode::Editing(mut editor) => {
            let editor_key = match key.code {
                KeyCode::Enter => EditorKey::Enter,
                KeyCode::Esc => EditorKey::Cancel,
                KeyCode::Backspace => EditorKey::Backspace,
                KeyCode::Left => EditorKey::Left,
                KeyCode::Right => EditorKey::Right,
                KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    EditorKey::Char(character)
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    EditorKey::Cancel
                }
                _ => {
                    app.mode = Mode::Editing(editor);
                    return Ok(true);
                }
            };
            match editor.apply(editor_key) {
                EditorOutcome::Continue => {
                    app.mode = Mode::Editing(editor);
                }
                EditorOutcome::Cancel => {
                    app.mode = Mode::Reading;
                }
                EditorOutcome::Save(text) => {
                    let pid = editor.paragraph.clone().unwrap_or_default();
                    let content = if text.trim().is_empty() { None } else { Some(text) };
                    match (pid.is_empty(), app.reader.document_id.clone()) {
                        (false, Some(document_id)) => {
                            notes::upsert_note(&mut app.reader.marks, &document_id, &pid, content.as_deref());
                            app.save_marks();
                            let message = if content.is_some() {
                                format!("note saved for ¶{pid}")
                            } else {
                                format!("note removed from ¶{pid}")
                            };
                            app.set_message(message);
                        }
                        _ => app.set_message("no note target here"),
                    }
                    let theme = app.theme.clone();
                    let options = current_options(app);
                    app.reader.rebuild(&theme, options);
                    app.mode = Mode::Reading;
                }
            }
            return Ok(true);
        }
        Mode::Searching(buffer) => {
            match key.code {
                KeyCode::Enter => {
                    app.reader.search(&buffer);
                    app.mode = Mode::Reading;
                }
                KeyCode::Esc => {
                    app.mode = Mode::Reading;
                }
                KeyCode::Backspace => {
                    let mut next = buffer;
                    next.pop();
                    app.mode = Mode::Searching(next);
                }
                KeyCode::Char(character) => {
                    let mut next = buffer;
                    next.push(character);
                    app.mode = Mode::Searching(next);
                }
                _ => {}
            }
            return Ok(true);
        }
        Mode::Palette => {
            if let KeyCode::Char(character) = key.code {
                if let Some(color) = character.to_digit(10) {
                    if (1..=6).contains(&color) {
                        apply_color(app, i64::from(color));
                    }
                }
            }
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                app.mode = Mode::Reading;
            }
            return Ok(true);
        }
        Mode::References => {
            app.mode = Mode::Reading;
            return Ok(true);
        }
        Mode::Reading => {}
    }

    let viewport = terminal_viewport();
    if app.focus_sidebar {
        match key.code {
            KeyCode::Char('b') | KeyCode::Esc => {
                app.sidebar.toggle();
                app.sidebar.visible = false;
                app.focus_sidebar = false;
            }
            KeyCode::Tab => {
                app.sidebar.next_tab();
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let items = sidebar_items(app);
                app.sidebar.select(app.sidebar.selected + 1, items.len());
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let selected = app.sidebar.selected.saturating_sub(1);
                let items = sidebar_items(app);
                app.sidebar.select(selected, items.len());
            }
            KeyCode::Enter => {
                let items = sidebar_items(app);
                if let Some(item) = items.get(app.sidebar.selected).cloned() {
                    if let Some(anchor) = item.anchor {
                        app.reader.jump_to(&anchor);
                        app.focus_sidebar = false;
                    } else if let Some(target) = item.target {
                        navigate(app, &target).await?;
                    }
                }
            }
            KeyCode::Char('q') => return Ok(false),
            _ => {}
        }
        return Ok(true);
    }

    match key.code {
        KeyCode::Char('q') => return Ok(false),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(false),
        KeyCode::Char('j') | KeyCode::Down => app.reader.scroll_by(1, viewport),
        KeyCode::Char('k') | KeyCode::Up => app.reader.scroll_by(-1, viewport),
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.reader.scroll_by(viewport as isize / 2, viewport)
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.reader.scroll_by(-(viewport as isize / 2), viewport)
        }
        KeyCode::PageDown | KeyCode::Char(' ') => app.reader.scroll_by(viewport as isize, viewport),
        KeyCode::PageUp => app.reader.scroll_by(-(viewport as isize), viewport),
        KeyCode::Char('G') => app.reader.bottom(viewport),
        KeyCode::Char('g') => {
            if app.pending_g {
                app.reader.top();
                app.pending_g = false;
            } else {
                app.pending_g = true;
            }
        }
        KeyCode::Char('J') => app.reader.move_block(1),
        KeyCode::Char('K') => app.reader.move_block(-1),
        KeyCode::Char('h') | KeyCode::Left => app.reader.move_cursor_token(-1),
        KeyCode::Char('l') | KeyCode::Right => app.reader.move_cursor_token(1),
        KeyCode::Char('v') => {
            if app.reader.selection.is_some() {
                app.reader.cancel_selection();
            } else {
                app.reader.begin_selection();
            }
        }
        KeyCode::Esc => app.reader.cancel_selection(),
        KeyCode::Char(digit @ '1'..='6') => {
            if app.reader.selection.is_some() {
                if let Some(color) = digit.to_digit(10) {
                    apply_color(app, i64::from(color));
                }
            } else {
                app.set_message("press v first to select text");
            }
        }
        KeyCode::Char('0') | KeyCode::Char('x') => {
            if app.reader.selection.is_some() {
                if app.reader.clear_color().is_some() {
                    app.save_marks();
                    let theme = app.theme.clone();
                    let options = current_options(app);
                    app.reader.rebuild(&theme, options);
                    app.set_message("highlight removed");
                }
            }
        }
        KeyCode::Char('p') => {
            if app.reader.selection.is_some() {
                app.mode = Mode::Palette;
            }
        }
        KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.reader.next_hit();
            app.focus_sidebar = false;
        }
        KeyCode::Char('n') => {
            let pid = app.reader.current_pid().unwrap_or_default();
            let existing = app
                .reader
                .document_id
                .clone()
                .map(|document_id| {
                    notes::notes_for(&app.reader.marks, &document_id, &pid)
                        .first()
                        .map(|note| note.content.clone())
                        .unwrap_or_default()
                })
                .unwrap_or_default();
            app.mode = Mode::Editing(NoteEditor::open(&pid, &existing));
        }
        KeyCode::Char('b') => {
            app.sidebar.toggle();
            app.focus_sidebar = app.sidebar.visible;
            app.resize(100);
        }
        KeyCode::Char('f') => app.mode = Mode::References,
        KeyCode::Char('/') => app.mode = Mode::Searching(String::new()),
        KeyCode::Char('N') => app.reader.previous_hit(),
        KeyCode::Char('r') => refresh_current(app).await?,
        KeyCode::Enter => {
            if let Some(link) = current_link(app) {
                navigate(app, &link).await?;
            }
        }
        KeyCode::Char('L') => {
            if app.history_index + 1 < app.history.len() {
                app.history_index += 1;
                let target = app.history[app.history_index].clone();
                navigate_to(app, &target).await?;
            }
        }
        KeyCode::Char('H') => {
            if app.history_index > 0 {
                app.history_index -= 1;
                let target = app.history[app.history_index].clone();
                navigate_to(app, &target).await?;
            }
        }
        _ => {}
    }
    Ok(true)
}

fn current_options(_app: &App) -> RenderOptions {
    RenderOptions {
        width: 96,
        gutter: true,
        show_notes: true,
        show_footnotes: true,
    }
}

fn apply_color(app: &mut App, color: i64) {
    if app.reader.apply_color(color).is_some() {
        app.save_marks();
        let theme = app.theme.clone();
        let options = current_options(app);
        app.reader.rebuild(&theme, options);
        app.set_message(format!(
            "applied {}",
            crate::render::HighlightColor::label(color)
        ));
    } else {
        app.set_message("nothing selected");
    }
    app.mode = Mode::Reading;
}

fn sidebar_items(app: &App) -> Vec<sidebar::SidebarItem> {
    let cached = app
        .context
        .cache
        .list_entries(60, 0, Some("/wol/"))
        .unwrap_or_default();
    app.sidebar
        .items(&cached, Some(&app.reader.document), &app.reader.marks)
}

fn current_link(app: &App) -> Option<String> {
    let block = app.reader.current_block()?;
    let token = block.tokens.get(usize::try_from(app.reader.cursor_token).ok()?)?;
    let index = block
        .pieces
        .iter()
        .position(|piece| piece.start <= token.start && token.start < piece.end)?;
    block.pieces.get(index).and_then(|piece| piece.href.clone())
}

async fn navigate(app: &mut App, target: &str) -> io::Result<()> {
    let target = target.to_string();
    app.history.truncate(app.history_index + 1);
    app.history.push(target.clone());
    app.history_index = app.history.len() - 1;
    navigate_to(app, &target).await
}

async fn navigate_to(app: &mut App, target: &str) -> io::Result<()> {
    match load(&app.context, target, false, false).await {
        Ok((document, canonical)) => {
            app.reader = Reader::new(
                document,
                app.reader.marks.clone(),
                &app.theme,
                current_options(app),
            );
            app.current = canonical;
            app.set_message(format!("opened {}", app.current));
        }
        Err(err) => app.set_message(format!("could not open {target}: {err}")),
    }
    Ok(())
}

/// Re-fetch the current page from the network, keeping local marks.
async fn refresh_current(app: &mut App) -> io::Result<()> {
    let target = app.current.clone();
    match load(&app.context, &target, false, true).await {
        Ok((document, canonical)) => {
            app.reader = Reader::new(
                document,
                app.reader.marks.clone(),
                &app.theme,
                current_options(app),
            );
            app.current = canonical;
            app.set_message("reloaded from the network");
        }
        Err(err) => app.set_message(format!("refresh failed: {err}")),
    }
    Ok(())
}

fn terminal_viewport() -> usize {
    crossterm::terminal::size()
        .map(|(_, rows)| usize::from(rows).saturating_sub(4))
        .unwrap_or(20)
}

fn draw(frame: &mut ratatui::Frame<'_>, app: &App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(3), Constraint::Length(1)])
        .split(area);

    let title = format!(
        " OfflineWOL  {} ",
        app.reader
            .document
            .title
            .clone()
            .unwrap_or_else(|| app.current.clone())
    );
    frame.render_widget(
        Paragraph::new(TuiLine::from(vec![TuiSpan::styled(
            title,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )])),
        chunks[0],
    );

    let body_area = chunks[1];
    if app.sidebar.visible {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(30), Constraint::Min(20)])
            .split(body_area);
        draw_sidebar(frame, app, columns[0]);
        draw_reader(frame, app, columns[1]);
    } else {
        draw_reader(frame, app, body_area);
    }

    let status = app.reader.status(&app.theme);
    let hint = app
        .message
        .clone()
        .unwrap_or_else(|| "j/k scroll · v select · 1-6 highlight · n note · b sidebar · / search · q quit".into());
    frame.render_widget(
        Paragraph::new(TuiLine::from(vec![
            TuiSpan::styled(status, Style::default().fg(Color::DarkGray)),
            TuiSpan::raw("  "),
            TuiSpan::styled(hint, Style::default().fg(Color::Gray)),
        ])),
        chunks[2],
    );

    match &app.mode {
        Mode::Editing(editor) => draw_editor(frame, area, editor),
        Mode::Searching(buffer) => draw_search(frame, area, buffer),
        Mode::Palette => draw_palette(frame, area, app),
        Mode::References => draw_references(frame, area, app),
        Mode::Reading => {}
    }
}

fn draw_reader(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let viewport = usize::from(area.height.saturating_sub(2));
    let lines: Vec<TuiLine<'_>> = app
        .reader
        .viewport(viewport)
        .iter()
        .map(|line| ansi_to_tui_line(&line.text))
        .collect();
    let widget = Paragraph::new(lines)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: false });
    frame.render_widget(widget, area);
}

fn draw_sidebar(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let items = sidebar_items(app);
    let list: Vec<ListItem<'_>> = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let style = if index == app.sidebar.selected {
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(item.label.clone()).style(style)
        })
        .collect();
    let title = format!(" {}  (tab) ", app.sidebar.tab.title());
    frame.render_widget(
        List::new(list).block(Block::default().borders(Borders::ALL).title(title)),
        area,
    );
}

fn draw_editor(frame: &mut ratatui::Frame<'_>, area: Rect, editor: &NoteEditor) {
    let popup = centered(area, 60, 40);
    frame.render_widget(Clear, popup);
    let (before, after) = editor.parts();
    let text = vec![
        TuiLine::from(vec![
            TuiSpan::raw(before),
            TuiSpan::styled("▏", Style::default().fg(Color::Cyan)),
            TuiSpan::raw(after),
        ]),
        TuiLine::from(""),
        TuiLine::from(vec![TuiSpan::styled(
            "Enter save · Esc cancel · empty removes the note",
            Style::default().fg(Color::DarkGray),
        )]),
    ];
    let paragraph = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" ✎ Note ¶{} ", editor.paragraph.clone().unwrap_or_default())),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, popup);
}

fn draw_search(frame: &mut ratatui::Frame<'_>, area: Rect, buffer: &str) {
    let popup = Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(3),
        width: area.width,
        height: 3,
    };
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(format!("/{buffer}")).block(Block::default().borders(Borders::ALL).title(" search ")),
        popup,
    );
}

fn draw_palette(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
    let popup = centered(area, 40, 50);
    frame.render_widget(Clear, popup);
    let mut lines = vec![TuiLine::from("press 1-6 to apply, Esc to cancel")];
    for entry in app.reader.legend(&app.theme) {
        lines.push(ansi_to_tui_line(&entry));
    }
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" colour ")),
        popup,
    );
}

fn draw_references(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
    let popup = Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(12),
        width: area.width,
        height: 12.min(area.height),
    };
    frame.render_widget(Clear, popup);
    let mut lines: Vec<TuiLine<'_>> = vec![TuiLine::from(format!(
        "¶{} · {} tokens",
        app.reader.current_pid().unwrap_or_default(),
        app.reader.tokens_of_current_block().len()
    ))];
    for note in app
        .reader
        .document_id
        .clone()
        .map(|document_id| {
            notes::notes_for(
                &app.reader.marks,
                &document_id,
                &app.reader.current_pid().unwrap_or_default(),
            )
        })
        .unwrap_or_default()
    {
        lines.push(TuiLine::from(format!("✎ {}", note.content)));
    }
    lines.push(TuiLine::from("press any key to close"));
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title(" references & notes "))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn centered(area: Rect, width_pct: u16, height_pct: u16) -> Rect {
    let width = area.width * width_pct / 100;
    let height = area.height * height_pct / 100;
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

/// Current SGR state while translating one line of ANSI-styled text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Sgr {
    fg: Option<Color>,
    bg: Option<Color>,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
}

impl Sgr {
    fn style(self) -> Style {
        let mut style = Style::default();
        if let Some(fg) = self.fg {
            style = style.fg(fg);
        }
        if let Some(bg) = self.bg {
            style = style.bg(bg);
        }
        if self.bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        if self.dim {
            style = style.add_modifier(Modifier::DIM);
        }
        if self.italic {
            style = style.add_modifier(Modifier::ITALIC);
        }
        if self.underline {
            style = style.add_modifier(Modifier::UNDERLINED);
        }
        style
    }
}

/// Convert the SGR sequences produced by [`Theme`] into ratatui styling.
///
/// The renderer paints highlights with 24-bit background colours; dropping the
/// escapes (as the old `strip_ansi` did) hid every highlight in the TUI.
fn ansi_to_tui_line(text: &str) -> TuiLine<'static> {
    let mut spans: Vec<TuiSpan<'static>> = Vec::new();
    let mut state = Sgr::default();
    let mut pending = String::new();
    let mut chars = text.chars();

    while let Some(character) = chars.next() {
        if character != '\x1b' {
            pending.push(character);
            continue;
        }
        let mut sequence = String::new();
        for next in chars.by_ref() {
            if next == 'm' {
                break;
            }
            sequence.push(next);
        }
        if !pending.is_empty() {
            spans.push(TuiSpan::styled(std::mem::take(&mut pending), state.style()));
        }
        apply_sgr(&mut state, &sequence);
    }
    if !pending.is_empty() {
        spans.push(TuiSpan::styled(pending, state.style()));
    }
    TuiLine::from(spans)
}

/// Apply one SGR parameter list (`"0"`, `"48;2;1;2;3"`, …) to `state`.
fn apply_sgr(state: &mut Sgr, sequence: &str) {
    // The terminal writes CSI sequences as `ESC [ params m`; only the params matter here.
    let sequence = sequence.strip_prefix('[').unwrap_or(sequence);
    let params: Vec<Option<u32>> = sequence
        .split(';')
        .map(|part| part.parse::<u32>().ok())
        .collect();
    let mut index = 0;
    while index < params.len() {
        let Some(code) = params[index] else {
            index += 1;
            continue;
        };
        index += 1;
        match code {
            0 => *state = Sgr::default(),
            1 => state.bold = true,
            2 => state.dim = true,
            3 => state.italic = true,
            4 => state.underline = true,
            22 => {
                state.bold = false;
                state.dim = false;
            }
            23 => state.italic = false,
            24 => state.underline = false,
            39 => state.fg = None,
            49 => state.bg = None,
            30..=37 => state.fg = Some(Color::Indexed((code - 30) as u8)),
            90..=97 => state.fg = Some(Color::Indexed((code - 90 + 8) as u8)),
            40..=47 => state.bg = Some(Color::Indexed((code - 40) as u8)),
            100..=107 => state.bg = Some(Color::Indexed((code - 100 + 8) as u8)),
            38 | 48 => {
                let color = match params.get(index) {
                    Some(Some(5)) => params.get(index + 1).and_then(|value| *value).map(|value| {
                        index += 2;
                        Color::Indexed(value.min(255) as u8)
                    }),
                    Some(Some(2)) => {
                        let rgb = (
                            params.get(index + 1).and_then(|value| *value),
                            params.get(index + 2).and_then(|value| *value),
                            params.get(index + 3).and_then(|value| *value),
                        );
                        match rgb {
                            (Some(r), Some(g), Some(b)) => {
                                index += 4;
                                Some(Color::Rgb(r.min(255) as u8, g.min(255) as u8, b.min(255) as u8))
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                };
                if code == 38 {
                    state.fg = color.or(state.fg);
                } else {
                    state.bg = color.or(state.bg);
                }
            }
            _ => {}
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn styles(line: &TuiLine<'_>) -> Vec<(String, Style)> {
        line.spans
            .iter()
            .map(|span| (span.content.to_string(), span.style))
            .collect()
    }

    #[test]
    fn plain_text_becomes_a_single_unstyled_span() {
        let line = ansi_to_tui_line("just text");
        assert_eq!(styles(&line), vec![("just text".to_string(), Style::default())]);
    }

    #[test]
    fn highlight_background_survives_conversion() {
        let theme = Theme::new(ColorDepth::TrueColor, Glyphs::Unicode);
        let painted = theme.paint(crate::render::StyleRole::Highlight(1), "verse");
        let line = ansi_to_tui_line(&painted);
        let spans = styles(&line);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].0, "verse");
        assert_eq!(spans[0].1.bg, Some(Color::Rgb(255, 227, 107)));
        assert_eq!(spans[0].1.fg, Some(Color::Rgb(0x10, 0x14, 0x1A)));
    }

    #[test]
    fn ansi256_and_attributes_are_translated() {
        let line = ansi_to_tui_line("\x1b[1;3;38;5;42malpha\x1b[0m beta");
        let spans = styles(&line);
        assert_eq!(spans[0].0, "alpha");
        assert_eq!(spans[0].1.fg, Some(Color::Indexed(42)));
        assert!(spans[0].1.add_modifier.contains(Modifier::BOLD));
        assert!(spans[0].1.add_modifier.contains(Modifier::ITALIC));
        assert_eq!(spans[1], (" beta".to_string(), Style::default()));
    }

    #[test]
    fn reset_clears_previous_state() {
        let line = ansi_to_tui_line("\x1b[48;2;1;2;3mone\x1b[0mtwo");
        let spans = styles(&line);
        assert_eq!(spans[0].1.bg, Some(Color::Rgb(1, 2, 3)));
        assert_eq!(spans[1].1, Style::default());
    }

    #[test]
    fn malformed_sequences_are_ignored() {
        let line = ansi_to_tui_line("\x1b[mok\x1b[999m!");
        let text: String = styles(&line).into_iter().map(|(text, _)| text).collect();
        assert_eq!(text, "ok!");
        for (_, style) in styles(&line) {
            assert_eq!(style, Style::default());
        }
    }
}

