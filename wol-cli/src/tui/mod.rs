//! Full-screen interactive reader (ratatui + crossterm).
//!
//! Keys (Vim-flavoured):
//!   j/k, Ctrl-d/Ctrl-u, g/G  scroll         J/K      next/previous paragraph
//!   h/l, Left/Right          move token    v        visual selection
//!   1-6                      apply colour  0/x      remove highlight
//!   n                        edit note     b        sidebar    tab (in sidebar)
//!   / n N, Ctrl-n            search        f        references drawer
//!   Tab                      browse links  Enter    follow link
//!   Backspace, H/L           history       gh       home page
//!   Ctrl-l                   language      r        refresh       q quit

pub mod languages;
pub mod notes;
pub mod reader;
pub mod sidebar;

use std::io::{self, IsTerminal as _};
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
use wol_core::languages::{default_language, find_by_locale, find_by_symbol, LanguageInfo};
use wol_core::state::AppSettings;

use crate::context::Context;
use crate::render::article::RenderOptions;
use crate::render::{ColorDepth, Glyphs, Theme};
use crate::Cli;

use self::languages::LanguagePicker;
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
    Languages(LanguagePicker),
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
    /// `Tab` mode: `j`/`k` walk the page's links instead of the text.
    link_focus: bool,
    link_index: usize,
}

pub async fn run(
    cli: &Cli,
    context: &Context,
    target: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    if !io::stdout().is_terminal() {
        return Err("the interactive reader needs a terminal — use `wol read <target>` instead".into());
    }
    let settings = context.state.get_settings().unwrap_or_default();
    let target = match target {
        Some(target) => Some(target.to_string()),
        None => startup_target(&context, &settings),
    };

    let theme = Theme::new(ColorDepth::detect(cli.no_color), Glyphs::detect(cli.ascii));
    // Without a target there is no language configured yet: open the picker on an
    // empty page instead of failing, exactly like the desktop setup screen.
    let (document, current, mode, message) = match target {
        Some(target) => match load(&context, &target, cli.offline, false).await {
            Ok((document, canonical)) => (document, canonical, Mode::Reading, None),
            Err(err) => (
                parse_document(""),
                target.clone(),
                Mode::Reading,
                Some(format!("could not open {target}: {err} — Ctrl-l picks a language, gh the home page")),
            ),
        },
        None => (
            parse_document(""),
            String::new(),
            Mode::Languages(LanguagePicker::new()),
            Some("choose your language (type to filter, Enter to confirm)".to_string()),
        ),
    };
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
        mode,
        history: if current.is_empty() { Vec::new() } else { vec![current.clone()] },
        history_index: 0,
        current,
        message,
        pending_g: false,
        focus_sidebar: false,
        link_focus: false,
        link_index: 0,
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

/// Where `wol` / `wol tui` starts: the configured home page, else the home page
/// of the configured language, else `None` (first run → language picker).
fn startup_target(context: &Context, settings: &AppSettings) -> Option<String> {
    startup_target_for(settings, &context.base_host())
}

fn startup_target_for(settings: &AppSettings, base_host: &str) -> Option<String> {
    if !settings.home.trim().is_empty() {
        return Some(settings.home.clone());
    }
    let language = configured_language(settings)?;
    Some(language.home_url(base_host))
}

fn configured_language(settings: &AppSettings) -> Option<&'static LanguageInfo> {
    let value = settings.language.trim();
    if value.is_empty() {
        return None;
    }
    find_by_locale(value).or_else(|| find_by_symbol(value))
}

/// Open the configured home page (`gh`): settings first, then the language table.
async fn open_home(app: &mut App) -> io::Result<()> {
    let target = home_target(app);
    navigate(app, &target).await
}

/// `gh` / fallback target: settings home, else the configured language's home,
/// else the built-in default language (Spanish).
fn home_target(app: &App) -> String {
    let settings = app.context.state.get_settings().unwrap_or_default();
    startup_target_for(&settings, &app.context.base_host())
        .unwrap_or_else(|| default_language().home_url(&app.context.base_host()))
}

/// Persist the chosen language the way the desktop settings UI does, then open
/// that language's home page.
async fn choose_language(app: &mut App, language: &'static LanguageInfo) -> io::Result<()> {
    let mut settings = app.context.state.get_settings().unwrap_or_default();
    settings.base_host = Some(app.context.base_host());
    settings.language = language.locale.clone();
    settings.region = language.region;
    settings.home = language.home_url(&app.context.base_host());
    let home = settings.home.clone();
    if let Err(err) = app.context.state.set_settings(settings) {
        app.set_message(format!("could not save the language: {err}"));
        return Ok(());
    }
    app.mode = Mode::Reading;
    navigate(app, &home).await
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
        Mode::Languages(mut picker) => {
            match key.code {
                KeyCode::Esc => app.mode = Mode::Reading,
                KeyCode::Enter => {
                    if let Some(language) = picker.selected_language() {
                        app.mode = Mode::Reading;
                        choose_language(app, language).await?;
                        return Ok(true);
                    }
                    app.set_message("no language matches that filter");
                }
                KeyCode::Up => picker.move_by(-1),
                KeyCode::Down => picker.move_by(1),
                KeyCode::PageUp => picker.move_by(-(languages::PAGE as isize)),
                KeyCode::PageDown => picker.move_by(languages::PAGE as isize),
                KeyCode::Backspace => picker.pop(),
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => picker.clear(),
                KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    picker.push(character)
                }
                _ => {}
            }
            if !matches!(app.mode, Mode::Reading) {
                app.mode = Mode::Languages(picker);
            }
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

    // Link browsing (`Tab`): j/k walk the page's links, Enter follows them.
    if app.link_focus {
        match key.code {
            KeyCode::Tab | KeyCode::Esc | KeyCode::Char('b') => {
                app.link_focus = false;
                app.set_message("link browsing off");
                return Ok(true);
            }
            KeyCode::Char('q') => return Ok(false),
            KeyCode::Char('j') | KeyCode::Down => step_link(app, 1),
            KeyCode::Char('k') | KeyCode::Up => step_link(app, -1),
            KeyCode::Enter => {
                if let Some(link) = app.reader.links().get(app.link_index).cloned() {
                    app.link_focus = false;
                    navigate(app, &link.href).await?;
                }
                return Ok(true);
            }
            KeyCode::Backspace => {
                go_back(app).await?;
                return Ok(true);
            }
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
        KeyCode::Tab => enter_link_focus(app),
        KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.mode = Mode::Languages(LanguagePicker::new());
        }
        KeyCode::Backspace => go_back(app).await?,
        KeyCode::Char('h') if app.pending_g => {
            app.pending_g = false;
            open_home(app).await?;
        }
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
            } else {
                app.set_message("no link under the cursor — Tab browses links");
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

/// `Tab`: start browsing the page's links from wherever the viewport is.
fn enter_link_focus(app: &mut App) {
    if app.reader.links().is_empty() {
        app.set_message("this page has no links");
        return;
    }
    let index = app
        .reader
        .nearest_link()
        .or_else(|| app.reader.link_at_top())
        .unwrap_or(0);
    app.link_index = index;
    if let Some(link) = app.reader.focus_link(index) {
        app.link_focus = true;
        app.set_message(format!("link {}/{} · {} → {}", index + 1, app.reader.links().len(), label_of(&link), link.href));
    } else {
        app.set_message("this page has no links");
    }
}

fn step_link(app: &mut App, delta: isize) {
    let index = app.link_index;
    if let Some((next, link)) = app.reader.step_link(index, delta) {
        app.link_index = next;
        app.set_message(format!(
            "link {}/{} · {} → {}",
            next + 1,
            app.reader.links().len(),
            label_of(&link),
            link.href
        ));
    }
}

fn label_of(link: &wol_core::dom::Link) -> String {
    let text = link.text.trim();
    if text.is_empty() {
        link.href.clone()
    } else {
        text.chars().take(60).collect()
    }
}

async fn go_back(app: &mut App) -> io::Result<()> {
    if app.history_index == 0 {
        app.set_message("no earlier page in this session");
        return Ok(());
    }
    app.history_index -= 1;
    let target = app.history[app.history_index].clone();
    navigate_to(app, &target).await
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

    let mut status = app.reader.status(&app.theme);
    if app.link_focus {
        status.push_str(&format!(
            "  ·  link {}/{}",
            app.link_index + 1,
            app.reader.links().len()
        ));
    }
    let hint = app.message.clone().unwrap_or_else(|| {
        if app.link_focus {
            "j/k link · Enter open · Backspace back · Esc close".into()
        } else {
            "j/k scroll · Tab links · v select · 1-6 highlight · n note · b sidebar · gh home · q quit"
                .into()
        }
    });
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
        Mode::Languages(picker) => draw_languages(frame, area, picker),
        Mode::Reading => {}
    }
}

fn draw_reader(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let viewport = usize::from(area.height.saturating_sub(2));
    let marked = app
        .link_focus
        .then(|| app.reader.links().get(app.link_index))
        .flatten()
        .and_then(|link| app.reader.line_of_node(link.node));
    let marker = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let lines: Vec<TuiLine<'static>> = app
        .reader
        .viewport(viewport)
        .iter()
        .enumerate()
        .map(|(offset, line)| {
            let mut rendered = ansi_to_tui_line(&line.text);
            if marked == Some(app.reader.scroll + offset) {
                rendered
                    .spans
                    .insert(0, TuiSpan::styled("▸ ", marker));
            }
            rendered
        })
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

fn draw_languages(frame: &mut ratatui::Frame<'_>, area: Rect, picker: &LanguagePicker) {
    let popup = centered(area, 70, 70);
    frame.render_widget(Clear, popup);
    let mut lines: Vec<TuiLine<'_>> = vec![
        TuiLine::from(vec![
            TuiSpan::styled("filter: ", Style::default().fg(Color::DarkGray)),
            TuiSpan::raw(picker.filter.clone()),
            TuiSpan::styled("▏", Style::default().fg(Color::Cyan)),
        ]),
        TuiLine::from(""),
    ];
    if picker.visible().is_empty() {
        lines.push(TuiLine::from(vec![TuiSpan::styled(
            "no language matches — Backspace clears a character",
            Style::default().fg(Color::DarkGray),
        )]));
    }
    for (language, selected) in picker.visible() {
        let label = format!(
            "{:<6} {:<28} {}  ·  r{}",
            language.symbol, language.name, language.locale, language.region
        );
        let style = if selected {
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        lines.push(TuiLine::from(TuiSpan::styled(label, style)));
    }
    let (position, total) = picker.position();
    lines.push(TuiLine::from(""));
    lines.push(TuiLine::from(vec![TuiSpan::styled(
        format!("{position}/{total} · type to filter · ↑/↓ move · Enter confirm · Esc cancel"),
        Style::default().fg(Color::DarkGray),
    )]));
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title(" language "))
            .wrap(Wrap { trim: false }),
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
    fn startup_target_uses_the_configured_home_page() {
        let settings = AppSettings {
            language: "es".into(),
            home: "https://wol.jw.org/es/wol/library/r4/lp-s/biblioteca".into(),
            ..AppSettings::default()
        };
        assert_eq!(
            startup_target_for(&settings, "wol.jw.org").as_deref(),
            Some("https://wol.jw.org/es/wol/library/r4/lp-s/biblioteca")
        );
    }

    #[test]
    fn startup_target_falls_back_to_the_language_home() {
        let settings = AppSettings {
            language: "es".into(),
            ..AppSettings::default()
        };
        assert_eq!(
            startup_target_for(&settings, "wol.jw.org").as_deref(),
            Some("https://wol.jw.org/es/wol/h/r4/lp-s")
        );

        // A language symbol (not a locale) also resolves.
        let by_symbol = AppSettings {
            language: "S".into(),
            ..AppSettings::default()
        };
        assert!(startup_target_for(&by_symbol, "wol.jw.org")
            .is_some_and(|target| target.contains("/wol/h/")));
    }

    #[test]
    fn startup_target_is_none_on_a_fresh_install() {
        assert!(startup_target_for(&AppSettings::default(), "wol.jw.org").is_none());
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

