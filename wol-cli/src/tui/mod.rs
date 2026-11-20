//! Full-screen interactive reader (ratatui + crossterm).
//!
//! `wol` with no arguments opens the **home dashboard**: the same toolbar the
//! desktop app shows (`BIBLIA · PUBLICACIONES · REUNIONES · search · gear`), the
//! welcome panel and *today's* daily text. Arrows move, `Enter` opens, `Tab`
//! switches between the toolbar and the text, and everything else is the normal
//! reader.
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
//!
//! Home dashboard: `←`/`→` pick a button, `Enter` opens it, `↓`/`Tab` reads the
//! daily text, `Esc`/`Tab` back to the buttons, `[`/`]` previous/next day.
//!
//! Directory pages (library, meetings, Bible versions) open a two-pane browser:
//! `j/k` move through the sub-categories/cards on the left, `Enter`/`→` opens
//! the selection, `Esc`/`Backspace`/`←` go back.

pub mod browser;
pub mod home;
pub mod languages;
pub mod notes;
pub mod reader;
pub mod sidebar;

use std::io::{self, IsTerminal as _};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use unicode_width::UnicodeWidthChar;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line as TuiLine, Span as TuiSpan};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use ratatui::Terminal;

use wol_core::dom::{
    parse_chrome, parse_daily_text, parse_document, parse_nav, parse_welcome, Document, NavPage,
};
use wol_core::languages::{default_language, find_by_locale, find_by_symbol, LanguageInfo};
use wol_core::state::AppSettings;

use crate::context::Context;
use crate::render::article::{wrap_text, RenderOptions};
use crate::render::{ColorDepth, Glyphs, Theme};
use crate::Cli;

use self::browser::{Browser, Row, RowKind};
use self::home::{Action, Focus, Home};
use self::languages::LanguagePicker;
use self::notes::{EditorKey, EditorOutcome, NoteEditor};
use self::reader::Reader;
use self::sidebar::Sidebar;

/// Below this width the dashboard drops the welcome panel and shows only the
/// daily text, so an 80-column terminal still reads well.
const WIDE_LAYOUT: usize = 90;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    Reading,
    Editing(NoteEditor),
    Searching(String),
    SiteSearch(String),
    Palette,
    References,
    Settings(usize),
    Languages(LanguagePicker),
}

struct App {
    context: Context,
    theme: Theme,
    reader: Reader,
    sidebar: Sidebar,
    /// `Some` while the home dashboard is on screen.
    home: Option<Home>,
    /// `Some` while a WOL directory page is browsed (sub-categories + cards).
    browser: Option<Browser>,
    /// Remembered menu position per directory URL, so going back restores it.
    browser_positions: std::collections::HashMap<String, usize>,
    /// Cache totals captured when the settings sheet opens (avoids disk I/O per frame).
    settings_stats: Option<(u64, u64)>,
    mode: Mode,
    history: Vec<String>,
    history_index: usize,
    current: String,
    message: Option<String>,
    /// Body width of the last layout, so re-renders (highlight/note edits) keep
    /// the same wrapping instead of jumping back to a hard-coded width.
    width: usize,
    pending_g: bool,
    focus_sidebar: bool,
    /// `Tab` mode: `j`/`k` walk the page's links instead of the text.
    link_focus: bool,
    link_index: usize,
}

impl App {
    /// Rows of text the reader shows, enough for the cursor to stay visible.
    fn reader_height(&self) -> usize {
        // The terminal size is only known at draw time; 24 rows minus the
        // header and status line is the conservative default, and
        // `follow_cursor` clamps whatever is scrolled past the end.
        usize::from(crossterm::terminal::size().map(|(_, rows)| rows).unwrap_or(24)).saturating_sub(2)
    }
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
    let (page, current, mode, message) = match target {
        Some(target) => match load(&context, &target, cli.offline, false).await {
            Ok(page) => {
                let canonical = page.canonical.clone();
                (Some(page), canonical, Mode::Reading, None)
            }
            Err(err) => (
                None,
                target.clone(),
                Mode::Reading,
                Some(format!("could not open {target}: {err} — Ctrl-l picks a language, gh the home page")),
            ),
        },
        None => (
            None,
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
        header: true,
        show_notes: true,
        show_footnotes: true,
    };
    let reader = Reader::new(parse_document(""), marks, &theme, options);

    let mut app = App {
        context: context.clone(),
        theme,
        reader,
        sidebar: Sidebar::default(),
        home: None,
        browser: None,
        browser_positions: std::collections::HashMap::new(),
        settings_stats: None,
        mode,
        history: if current.is_empty() { Vec::new() } else { vec![current.clone()] },
        history_index: 0,
        current,
        message,
        width: 100,
        pending_g: false,
        focus_sidebar: false,
        link_focus: false,
        link_index: 0,
    };
    if let Some(page) = page {
        apply_page(&mut app, page);
    }
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

/// A fetched/cached page plus, when it is a WOL home page, its dashboard state.
struct LoadedPage {
    document: Document,
    canonical: String,
    home: Option<HomePage>,
    nav: Option<(NavPage, Option<String>)>,
}

struct HomePage {
    home: Home,
    /// Only today's section of the daily-text widget.
    daily: Document,
}

async fn load(
    context: &Context,
    target: &str,
    offline: bool,
    refresh: bool,
) -> Result<LoadedPage, Box<dyn std::error::Error>> {
    let page = if refresh {
        let canonical = context.canonicalize(target)?;
        context.fetch(canonical).await?
    } else {
        context.load(target, offline).await?
    };
    let canonical = page.canonical.as_str().to_string();
    let home = build_home(&page.html, &canonical);
    let chrome = parse_chrome(&page.html);
    let nav = parse_nav(&page.html).map(|nav| (nav, channel_label(&chrome, &canonical)));
    let mut document = parse_document(&page.html);
    document.source_url = Some(canonical.clone());
    Ok(LoadedPage {
        document,
        canonical,
        home,
        nav,
    })
}

/// Build the dashboard from a home page, or `None` for an ordinary document.
///
/// A page only becomes a dashboard when it carries both WOL's toolbar (`#menuBar`)
/// and the daily-text widget — that is exactly the desktop home screen.
/// Toolbar label of the section a URL belongs to (`BIBLIA`, `PUBLICACIONES`,
/// `REUNIONES`), read from WOL's own menu.
fn channel_label(chrome: &wol_core::dom::Chrome, canonical: &str) -> Option<String> {
    let wanted = section_key(canonical)?;
    chrome
        .menu
        .iter()
        .find(|entry| section_key(&entry.href).as_deref() == Some(wanted.as_str()))
        .map(|entry| entry.label.clone())
}

/// WOL's site section of a path: `/es/wol/meetings/r4/lp-s/2026/47` → `meetings`.
/// Comparing this instead of whole prefixes keeps a dated page (`…/meetings/2026/47`)
/// matched to the menu entry (`…/meetings`), and never confuses `binav` with `b`.
fn section_key(url: &str) -> Option<String> {
    let path = url.split(['?', '#']).next()?;
    let segments: Vec<&str> = path.split('/').filter(|segment| !segment.is_empty()).collect();
    // Site-relative hrefs start at the locale, absolute URLs at the scheme/host.
    let wol = segments.iter().position(|segment| *segment == "wol")?;
    segments.get(wol + 1).map(|section| (*section).to_string())
}

fn build_home(html: &str, canonical: &str) -> Option<HomePage> {
    let chrome = parse_chrome(html);
    if chrome.menu.is_empty() {
        return None;
    }
    // A dated page (`/h/…/2026/11/15`) must show that day; otherwise show today's.
    let day = home::daily_text_date(canonical).unwrap_or_else(home::today_prefix);
    let daily = parse_daily_text(html, Some(&day))?;
    let welcome = parse_welcome(html);
    Some(HomePage {
        home: Home::new(chrome, welcome, &daily),
        daily: daily.document,
    })
}

/// Put a freshly loaded page on screen: home pages open the dashboard, every
/// other page opens the reader.
fn apply_page(app: &mut App, page: LoadedPage) {
    app.current = page.canonical;
    app.home = None;
    app.browser = None;
    // Priority: WOL home dashboard → directory browser → plain reader.
    let document = match page.home {
        Some(HomePage { home, daily }) => {
            app.home = Some(home);
            daily
        }
        None => {
            if let Some((nav, channel)) = page.nav {
                app.browser = Browser::new(nav).map(|mut browser| {
                    browser.channel = channel;
                    if let Some(position) = app.browser_positions.get(&app.current) {
                        if browser.rows.get(*position).is_some_and(Row::is_selectable) {
                            browser.selected = *position;
                        }
                    }
                    browser
                });
            }
            page.document
        }
    };
    let marks = app.reader.marks.clone();
    let theme = app.theme.clone();
    let options = current_options(app);
    app.reader = Reader::new(document, marks, &theme, options);
    app.link_focus = false;
    app.link_index = 0;
    app.focus_sidebar = false;
    app.resize(terminal_width().unwrap_or(100));
}

/// Keep the browser's cursor so `Backspace` returns to the same sub-category.
fn remember_browser_position(app: &mut App) {
    if let Some(browser) = app.browser.as_ref() {
        app.browser_positions
            .insert(app.current.clone(), browser.selected);
    }
}

fn terminal_width() -> Option<usize> {
    crossterm::terminal::size()
        .ok()
        .map(|(columns, _)| usize::from(columns))
}

impl App {
    fn resize(&mut self, width: usize) {
        let header = self.home.is_none();
        let body = if self.home.is_some() {
            // The dashboard splits the body into welcome + daily-text panes.
            let daily = if width >= WIDE_LAYOUT {
                (width * 62 / 100).max(20)
            } else {
                width
            };
            daily.saturating_sub(3).max(20)
        } else {
            let sidebar_width = if self.sidebar.visible { 30 } else { 0 };
            width.saturating_sub(sidebar_width + 4).max(20)
        };
        self.width = body;
        self.reader.rebuild(
            &self.theme,
            RenderOptions {
                width: body,
                gutter: true,
                header,
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
        match event::read()? {
            Event::Key(key) => {
                if key.kind == event::KeyEventKind::Release {
                    continue;
                }
                if !handle_key(app, key).await? {
                    return Ok(());
                }
            }
            Event::Resize(width, _) => app.resize(usize::from(width)),
            _ => {}
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
        Mode::SiteSearch(buffer) => {
            match key.code {
                KeyCode::Enter if !buffer.trim().is_empty() => {
                    let action = app
                        .home
                        .as_ref()
                        .and_then(Home::search_action)
                        .unwrap_or_else(|| default_search_action(app));
                    let target = home::search_url(&action, &buffer);
                    app.mode = Mode::Reading;
                    navigate(app, &target).await?;
                }
                KeyCode::Enter => app.set_message("type a word or a scripture reference to search"),
                KeyCode::Esc => app.mode = Mode::Reading,
                KeyCode::Backspace => {
                    let mut next = buffer;
                    next.pop();
                    app.mode = Mode::SiteSearch(next);
                }
                KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    let mut next = buffer;
                    next.push(character);
                    app.mode = Mode::SiteSearch(next);
                }
                _ => {}
            }
            return Ok(true);
        }
        Mode::Settings(row) => {
            const ROWS: usize = 3;
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => app.mode = Mode::Reading,
                KeyCode::Up | KeyCode::Char('k') => {
                    app.mode = Mode::Settings(row.saturating_sub(1).min(ROWS - 1));
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    app.mode = Mode::Settings((row + 1).min(ROWS - 1));
                }
                KeyCode::Enter => match row {
                    0 => app.mode = Mode::Languages(LanguagePicker::new()),
                    1 => {
                        app.mode = Mode::Reading;
                        open_home(app).await?;
                    }
                    _ => app.mode = Mode::Reading,
                },
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

    // Directory browser: the left menu owns the arrows.
    if app.browser.is_some() {
        return handle_browser_key(app, key).await;
    }

    // Home dashboard: the toolbar owns the arrows until the daily text is focused.
    if let Some(home) = app.home.as_ref() {
        if home.focus == Focus::Menu {
            return handle_menu_key(app, key).await;
        }
        // In the text pane `Tab`/`Esc` hop back to the toolbar, while every other
        // key keeps its usual reader meaning (scroll, select, highlight, note).
        match key.code {
            KeyCode::Tab => {
                app.reader.cancel_selection();
                if let Some(home) = app.home.as_mut() {
                    home.focus_menu();
                }
                return Ok(true);
            }
            KeyCode::Esc => {
                if app.reader.selection.is_some() {
                    app.reader.cancel_selection();
                } else if let Some(home) = app.home.as_mut() {
                    home.focus_menu();
                }
                return Ok(true);
            }
            KeyCode::Char('b') => {
                app.set_message("the sidebar belongs to document pages — Tab returns to the toolbar");
                return Ok(true);
            }
            KeyCode::Char('[') if app.home.as_ref().is_some_and(|home| home.previous.is_some()) => {
                let target = app
                    .home
                    .as_ref()
                    .and_then(|home| home.previous.clone())
                    .unwrap_or_default();
                navigate(app, &target).await?;
                return Ok(true);
            }
            KeyCode::Char(']') if app.home.as_ref().is_some_and(|home| home.next.is_some()) => {
                let target = app
                    .home
                    .as_ref()
                    .and_then(|home| home.next.clone())
                    .unwrap_or_default();
                navigate(app, &target).await?;
                return Ok(true);
            }
            _ => {}
        }
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
        KeyCode::Char('j') | KeyCode::Down => app.reader.move_line(1, viewport),
        KeyCode::Char('k') | KeyCode::Up => app.reader.move_line(-1, viewport),
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.reader.move_line((viewport as isize / 2).max(1), viewport)
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.reader.move_line(-(viewport as isize / 2).max(1), viewport)
        }
        KeyCode::PageDown | KeyCode::Char(' ') => app.reader.move_line(viewport as isize, viewport),
        KeyCode::PageUp => app.reader.move_line(-(viewport as isize), viewport),
        KeyCode::Char('G') | KeyCode::End => app.reader.bottom(viewport),
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
        KeyCode::Char('h') | KeyCode::Left => {
            app.reader.move_cursor_token(-1);
            let height = app.reader_height();
            app.reader.follow_token(height);
        }
        KeyCode::Char('l') | KeyCode::Right => {
            app.reader.move_cursor_token(1);
            let height = app.reader_height();
            app.reader.follow_token(height);
        }
        KeyCode::Char('v') => {
            if app.reader.selection.is_some() {
                app.reader.cancel_selection();
                app.set_message("selection cancelled");
            } else {
                app.reader.begin_selection();
                if app.reader.selection.is_some() {
                    app.set_message("VISUAL — ←/→ extend · 1-6 colour · Esc cancel");
                } else {
                    app.set_message("move the cursor onto a paragraph to select text");
                }
            }
        }
        KeyCode::Esc => app.reader.cancel_selection(),
        KeyCode::Home => app.reader.top(),
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
            if pid.is_empty() {
                app.set_message("no paragraph here — ↑/↓ onto a paragraph first");
                return Ok(true);
            }
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

fn current_options(app: &App) -> RenderOptions {
    RenderOptions {
        width: app.width.max(20),
        gutter: true,
        header: true,
        show_notes: true,
        show_footnotes: true,
    }
}

/// Arrow/key handling for WOL directory pages (left menu + right content).
async fn handle_browser_key(app: &mut App, key: KeyEvent) -> io::Result<bool> {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('c') => return Ok(false),
            KeyCode::Char('l') => app.mode = Mode::Languages(LanguagePicker::new()),
            _ => {}
        }
        return Ok(true);
    }
    match key.code {
        KeyCode::Char('q') => return Ok(false),
        KeyCode::Char('j') | KeyCode::Down => step_browser(app, 1),
        KeyCode::Char('k') | KeyCode::Up => step_browser(app, -1),
        KeyCode::Char('g') | KeyCode::Home => {
            if let Some(browser) = app.browser.as_mut() {
                browser.first();
            }
        }
        KeyCode::Char('G') | KeyCode::End => {
            if let Some(browser) = app.browser.as_mut() {
                browser.last();
            }
        }
        KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => open_browser_item(app).await?,
        KeyCode::Backspace | KeyCode::Esc | KeyCode::Char('h') | KeyCode::Left => {
            go_back(app).await?
        }
        KeyCode::Char('/') => app.mode = Mode::SiteSearch(String::new()),
        KeyCode::Char('r') => refresh_current(app).await?,
        _ => {}
    }
    Ok(true)
}

fn step_browser(app: &mut App, delta: isize) {
    let Some(browser) = app.browser.as_mut() else {
        return;
    };
    browser.step(delta);
    let message = browser.selected_item().map(|(_, section, _, item)| {
        let section = section.title.clone().unwrap_or_default();
        let detail = item.detail.clone().unwrap_or_else(|| item.href.clone());
        format!("{section} · {} — {detail}", item.label)
    });
    if let Some(message) = message {
        app.set_message(message);
    }
}

async fn open_browser_item(app: &mut App) -> io::Result<()> {
    let target = app
        .browser
        .as_ref()
        .and_then(Browser::selected_item)
        .map(|(_, _, _, item)| item.href.clone());
    match target {
        Some(target) => navigate(app, &target).await?,
        None => app.set_message("nothing selected"),
    }
    Ok(())
}

/// Arrow/key handling while the dashboard toolbar has focus.
async fn handle_menu_key(app: &mut App, key: KeyEvent) -> io::Result<bool> {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('c') => return Ok(false),
            KeyCode::Char('l') => app.mode = Mode::Languages(LanguagePicker::new()),
            _ => {}
        }
        return Ok(true);
    }
    match key.code {
        KeyCode::Char('q') => return Ok(false),
        KeyCode::Left | KeyCode::Char('h') => step_home_menu(app, -1),
        KeyCode::Right | KeyCode::Char('l') => step_home_menu(app, 1),
        KeyCode::Down | KeyCode::Tab | KeyCode::Char('j') => {
            if let Some(home) = app.home.as_mut() {
                home.focus_text();
            }
            app.set_message(
                "j/k scroll · v select · 1-6 highlight · n note · Tab back to the toolbar",
            );
            return Ok(true);
        }
        KeyCode::Enter | KeyCode::Char(' ') => activate_home_button(app).await?,
        KeyCode::Char('/') => {
            if let Some(home) = app.home.as_mut() {
                home.select_action(|action| matches!(action, Action::Search));
            }
            app.mode = Mode::SiteSearch(String::new());
        }
        KeyCode::Char('r') => refresh_current(app).await?,
        KeyCode::Backspace => go_back(app).await?,
        _ => {}
    }
    Ok(true)
}

fn step_home_menu(app: &mut App, delta: isize) {
    let Some(home) = app.home.as_mut() else {
        return;
    };
    home.step(delta);
    if let Some(button) = home.selected() {
        let label = button.label.clone();
        let hint = match &button.action {
            Action::Open(target) => target.clone(),
            Action::Search => "site search".to_string(),
            Action::Settings => "settings".to_string(),
        };
        app.set_message(format!("{label} — Enter opens {hint}"));
    }
}

async fn activate_home_button(app: &mut App) -> io::Result<()> {
    let Some(button) = app.home.as_ref().and_then(Home::selected).cloned() else {
        return Ok(());
    };
    match button.action {
        // A failed load keeps the dashboard (and its toolbar) on screen.
        Action::Open(target) => navigate(app, &target).await?,
        Action::Search => app.mode = Mode::SiteSearch(String::new()),
        Action::Settings => open_settings(app),
    }
    Ok(())
}

fn open_settings(app: &mut App) {
    let entries = app.context.cache.entry_count().unwrap_or(0);
    let bytes = app.context.cache.total_size().unwrap_or(0);
    app.settings_stats = Some((entries, bytes));
    app.mode = Mode::Settings(0);
}

/// Search endpoint for pages whose chrome has no search form.
fn default_search_action(app: &App) -> String {
    let settings = app.context.state.get_settings().unwrap_or_default();
    let language = configured_language(&settings);
    let locale = if settings.language.trim().is_empty() {
        language.map(|info| info.locale.clone()).unwrap_or_else(|| "en".to_string())
    } else {
        settings.language.clone()
    };
    let region = if settings.region == 0 {
        language.map(|info| info.region).unwrap_or(1)
    } else {
        settings.region
    };
    let library = language
        .map(|info| info.lib.clone())
        .unwrap_or_else(|| "lp-e".to_string());
    format!("/{locale}/wol/qt/r{region}/{library}")
}

fn apply_color(app: &mut App, color: i64) {
    let target = app.reader.selected_range();
    if app.reader.apply_color(color).is_some() {
        app.save_marks();
        let theme = app.theme.clone();
        let options = current_options(app);
        app.reader.rebuild(&theme, options);
        let label = crate::render::HighlightColor::label(color);
        app.set_message(match target {
            Some((_, start, end)) => format!("{label}: tokens {start}-{end}"),
            None => format!("applied {label}"),
        });
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
    remember_browser_position(app);
    match load(&app.context, target, false, false).await {
        Ok(page) => {
            apply_page(app, page);
            let current = app.current.clone();
            let label = if app.home.is_some() {
                "home dashboard"
            } else if app.browser.is_some() {
                "category"
            } else {
                "document"
            };
            app.set_message(format!("opened {label} · {current}"));
        }
        Err(err) => app.set_message(format!("could not open {target}: {err}")),
    }
    Ok(())
}

/// Re-fetch the current page from the network, keeping local marks.
async fn refresh_current(app: &mut App) -> io::Result<()> {
    remember_browser_position(app);
    let target = app.current.clone();
    match load(&app.context, &target, false, true).await {
        Ok(page) => {
            apply_page(app, page);
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
    if app.home.is_some() {
        draw_home(frame, app, area);
    } else if app.browser.is_some() {
        draw_browser(frame, app, area);
    } else {
        draw_document(frame, app, area);
    }

    match &app.mode {
        Mode::Editing(editor) => draw_editor(frame, area, editor),
        Mode::Searching(buffer) => draw_search(frame, area, buffer),
        Mode::SiteSearch(buffer) => draw_site_search(frame, area, buffer, app),
        Mode::Palette => draw_palette(frame, area, app),
        Mode::References => draw_references(frame, area, app),
        Mode::Settings(row) => draw_settings(frame, area, app, *row),
        Mode::Languages(picker) => draw_languages(frame, area, picker),
        Mode::Reading => {}
    }
}

/// Ordinary document view: title bar, article and status line.
/// Directory browser: sub-categories on the left, their cards on the right.
fn draw_browser(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let Some(browser) = app.browser.as_ref() else {
        return;
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(3), Constraint::Length(1)])
        .split(area);

    let title = browser
        .header_title()
        .unwrap_or_else(|| app.current.clone());
    let header = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(20), Constraint::Length(18)])
        .split(chunks[0]);
    frame.render_widget(
        Paragraph::new(TuiLine::from(vec![
            TuiSpan::styled(
                " OfflineWOL ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            TuiSpan::styled(format!("· {title}"), Style::default().fg(Color::DarkGray)),
        ])),
        header[0],
    );
    frame.render_widget(
        Paragraph::new(format!("{}/{} ", browser.item_ordinal(), browser.item_count()))
            .alignment(Alignment::Right),
        header[1],
    );

    if usize::from(chunks[1].width) >= WIDE_LAYOUT {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
            .split(chunks[1]);
        draw_browser_menu(frame, browser, columns[0]);
        draw_browser_content(frame, browser, columns[1]);
    } else {
        draw_browser_menu(frame, browser, chunks[1]);
    }

    let status = format!(
        "{}/{}",
        browser.item_ordinal(),
        browser.item_count()
    );
    let hint = app.message.clone().unwrap_or_else(|| {
        "j/k move · Enter open · Esc/Backspace back · / search · r refresh · q quit".to_string()
    });
    frame.render_widget(
        Paragraph::new(TuiLine::from(vec![
            TuiSpan::styled(status, Style::default().fg(Color::DarkGray)),
            TuiSpan::raw("  "),
            TuiSpan::styled(hint, Style::default().fg(Color::Gray)),
        ])),
        chunks[2],
    );
}

fn draw_browser_menu(frame: &mut ratatui::Frame<'_>, browser: &Browser, area: Rect) {
    let height = usize::from(area.height.saturating_sub(2));
    let width = usize::from(area.width.saturating_sub(3));
    if height == 0 {
        return;
    }
    let start = browser
        .selected
        .saturating_sub(height / 2)
        .min(browser.rows.len().saturating_sub(height));

    let mut lines: Vec<TuiLine<'static>> = Vec::new();
    for (index, row) in browser
        .rows
        .iter()
        .enumerate()
        .skip(start)
        .take(height)
    {
        match row.kind {
            RowKind::Section(_) => lines.push(TuiLine::from(TuiSpan::styled(
                format!(" {} ", clip(&row.label, width.saturating_sub(2))),
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            ))),
            RowKind::Item { .. } => {
                let selected = index == browser.selected;
                let text = format!(
                    "{} {}",
                    if selected { "▸" } else { " " },
                    clip(&row.label, width.saturating_sub(3))
                );
                let style = if selected {
                    Style::default()
                        .bg(Color::Cyan)
                        .fg(Color::Black)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                lines.push(TuiLine::from(TuiSpan::styled(text, style)));
            }
        }
    }
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    if let Some(title) = browser.page_title() {
        block = block.title(format!(" {title} "));
    }
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// Right pane: the cards of the selected sub-category, with the current one
/// expanded (title, subtitle and href).
fn draw_browser_content(frame: &mut ratatui::Frame<'_>, browser: &Browser, area: Rect) {
    let Some((_, section, item_index)) = browser.selected_section() else {
        return;
    };
    let inner_width = usize::from(area.width.saturating_sub(4)).max(10);
    let inner_height = usize::from(area.height.saturating_sub(2)).max(1);

    let title = section
        .title
        .clone()
        .or_else(|| browser.page_title());
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    if let Some(title) = title {
        block = block.title(format!(" {title} "));
    }

    // A page with one section *is* the list: repeating it on the right would just
    // mirror the menu, so show the selected card instead.
    if browser.page.sections.len() == 1 {
        let Some((_, _, item_index, item)) = browser.selected_item() else {
            return;
        };
        let mut lines: Vec<TuiLine<'static>> = vec![TuiLine::from("")];
        let selected = item_index;
        for text in wrap_text(&item.label, inner_width.saturating_sub(2)) {
            lines.push(TuiLine::from(TuiSpan::styled(
                text,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
        }
        if let Some(detail) = &item.detail {
            for text in wrap_text(detail, inner_width.saturating_sub(2)) {
                lines.push(TuiLine::from(TuiSpan::styled(
                    text,
                    Style::default().fg(Color::Gray),
                )));
            }
        }
        lines.push(TuiLine::from(""));
        lines.push(TuiLine::from(TuiSpan::styled(
            clip(&item.href, inner_width.saturating_sub(2)),
            Style::default().fg(Color::Blue),
        )));
        lines.push(TuiLine::from(""));
        lines.push(TuiLine::from(TuiSpan::styled(
            format!(
                "{}/{} · Enter opens this entry",
                selected + 1,
                section.items.len()
            ),
            Style::default().fg(Color::DarkGray),
        )));
        frame.render_widget(
            Paragraph::new(lines)
                .block(block)
                .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }

    let mut lines: Vec<TuiLine<'static>> = Vec::new();
    let mut selected_line = 0;
    for (index, item) in section.items.iter().enumerate() {
        let selected = index == item_index;
        if selected {
            selected_line = lines.len();
        }
        let marker = if selected { "▸ " } else { "  " };
        let label_style = if selected {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        for (line_index, text) in wrap_text(&item.label, inner_width.saturating_sub(2))
            .into_iter()
            .enumerate()
        {
            let lead = if line_index == 0 { marker } else { "  " };
            lines.push(TuiLine::from(vec![
                TuiSpan::styled(lead, Style::default().fg(Color::Cyan)),
                TuiSpan::styled(text, label_style),
            ]));
        }
        if let Some(detail) = &item.detail {
            for text in wrap_text(detail, inner_width.saturating_sub(4)) {
                lines.push(TuiLine::from(TuiSpan::styled(
                    format!("  {text}"),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
        if selected {
            lines.push(TuiLine::from(TuiSpan::styled(
                format!("  {}", clip(&item.href, inner_width.saturating_sub(2))),
                Style::default().fg(Color::Blue),
            )));
        }
        lines.push(TuiLine::from(""));
    }
    let offset = selected_line.saturating_sub(inner_height / 3);
    let visible: Vec<TuiLine<'static>> = lines
        .into_iter()
        .skip(offset)
        .take(inner_height)
        .collect();
    frame.render_widget(Paragraph::new(visible).block(block), area);
}

fn draw_document(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
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
            "j/k or ↑/↓ row · ←/→ char · v select · 1-6 highlight · n note · b sidebar · gh home · q quit"
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
}

/// Home dashboard: WOL's toolbar, the welcome panel and today's daily text.
fn draw_home(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let Some(home) = app.home.as_ref() else {
        return;
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(toolbar_height(home, area.width)),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(area);

    draw_home_header(frame, home, chunks[0]);
    draw_home_toolbar(frame, home, chunks[1]);

    let body = chunks[2];
    if usize::from(body.width) >= WIDE_LAYOUT {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
            .split(body);
        draw_welcome(frame, home, columns[0]);
        draw_daily_text(frame, app, home, columns[1]);
    } else {
        draw_daily_text(frame, app, home, body);
    }

    draw_home_status(frame, app, home, chunks[3]);
}

/// Rows the button strip needs: one per terminal width worth of labels.
fn toolbar_height(home: &Home, width: u16) -> u16 {
    let used: usize = home
        .buttons
        .iter()
        .map(|button| button.label.chars().count() + 3)
        .sum();
    let width = usize::from(width).max(1);
    u16::try_from(used.div_ceil(width).max(1)).unwrap_or(1)
}

fn draw_home_header(frame: &mut ratatui::Frame<'_>, home: &Home, area: Rect) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(20), Constraint::Length(24)])
        .split(area);
    let title = home.chrome.title.clone().unwrap_or_default();
    let mut spans = vec![TuiSpan::styled(
        " OfflineWOL ",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )];
    if !title.is_empty() {
        spans.push(TuiSpan::styled(
            format!("· {title}"),
            Style::default().fg(Color::DarkGray),
        ));
    }
    frame.render_widget(Paragraph::new(TuiLine::from(spans)), columns[0]);

    let language = home
        .chrome
        .language
        .as_ref()
        .map(|entry| entry.label.clone())
        .unwrap_or_default();
    frame.render_widget(
        Paragraph::new(format!("{language} ")).alignment(Alignment::Right),
        columns[1],
    );
}

/// The button strip. The arrow keys highlight one button at a time, exactly
/// like moving a mouse across the desktop toolbar.
fn draw_home_toolbar(frame: &mut ratatui::Frame<'_>, home: &Home, area: Rect) {
    let mut spans = vec![TuiSpan::raw(" ")];
    for (index, button) in home.buttons.iter().enumerate() {
        let current = index == home.menu_index;
        let style = if current && home.focus == Focus::Menu {
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else if current {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        spans.push(TuiSpan::styled(
            format!(" {} ", button.label),
            style,
        ));
        spans.push(TuiSpan::raw(" "));
    }
    frame.render_widget(
        Paragraph::new(TuiLine::from(spans)).wrap(Wrap { trim: false }),
        area,
    );
}

/// Left pane of the dashboard: the localised welcome/announcement panel.
fn draw_welcome(frame: &mut ratatui::Frame<'_>, home: &Home, area: Rect) {
    let title = home
        .welcome
        .as_ref()
        .and_then(|welcome| welcome.title.clone())
        .or_else(|| home.chrome.title.clone())
        .unwrap_or_default();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(format!(" {title} "));

    let width = usize::from(area.width.saturating_sub(4)).max(10);
    let mut lines: Vec<TuiLine<'static>> = Vec::new();
    if let Some(welcome) = &home.welcome {
        if let Some(brand) = &welcome.brand {
            lines.push(TuiLine::from(TuiSpan::styled(
                brand.clone(),
                Style::default().fg(Color::DarkGray),
            )));
        }
        if let Some(title) = &welcome.title {
            lines.push(TuiLine::from(TuiSpan::styled(
                title.clone(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
        }
        lines.push(TuiLine::from(""));
        if let Some(greeting) = &welcome.greeting {
            lines.push(TuiLine::from(TuiSpan::styled(
                greeting.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            )));
        }
        push_wrapped(&mut lines, welcome.about.as_deref(), width, Style::default());
        if welcome.notice.is_some() {
            lines.push(TuiLine::from(""));
        }
        push_wrapped(
            &mut lines,
            welcome.notice.as_deref(),
            width,
            Style::default().fg(Color::DarkGray),
        );
        if welcome.announcement.is_some() || welcome.message.is_some() {
            lines.push(TuiLine::from(""));
        }
        if let Some(announcement) = &welcome.announcement {
            lines.push(TuiLine::from(TuiSpan::styled(
                announcement.clone(),
                Style::default().fg(Color::DarkGray),
            )));
        }
        push_wrapped(
            &mut lines,
            welcome.message.as_deref(),
            width,
            Style::default().fg(Color::Gray),
        );
    }
    frame.render_widget(
        Paragraph::new(lines).block(block).wrap(Wrap { trim: false }),
        area,
    );
}

fn push_wrapped(
    lines: &mut Vec<TuiLine<'static>>,
    text: Option<&str>,
    width: usize,
    style: Style,
) {
    let Some(text) = text.map(str::trim).filter(|text| !text.is_empty()) else {
        return;
    };
    for line in wrap_text(text, width.max(10)) {
        lines.push(TuiLine::from(TuiSpan::styled(line, style)));
    }
}

/// Right pane of the dashboard: the daily text, editable and highlightable like
/// any other document because it *is* the reader.
fn draw_daily_text(frame: &mut ratatui::Frame<'_>, app: &App, home: &Home, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if home.focus == Focus::Text {
            Color::Cyan
        } else {
            Color::DarkGray
        }))
        .title(home.pane_title());
    let viewport = usize::from(area.height.saturating_sub(2));
    let lines = viewport_lines(app, viewport, None);
    frame.render_widget(
        Paragraph::new(lines).block(block).wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_home_status(frame: &mut ratatui::Frame<'_>, app: &App, home: &Home, area: Rect) {
    let position = format!("{}/{}", home.menu_index + 1, home.buttons.len());
    let status = match home.focus {
        Focus::Menu => match home.date.as_deref().map(|date| date.get(..10).unwrap_or(date)) {
            Some(day) => format!("{day} · {position}"),
            None => position,
        },
        Focus::Text => app.reader.status(&app.theme),
    };
    let hint = app.message.clone().unwrap_or_else(|| match home.focus {
        Focus::Menu => {
            "←/→ choose · Enter open · ↓/Tab read the daily text · / search · Ctrl-l language · q quit"
                .to_string()
        }
        Focus::Text => {
            let previous = home.label_previous.clone().unwrap_or_else(|| "previous".into());
            let next = home.label_next.clone().unwrap_or_else(|| "next".into());
            format!(
                "j/k or ↑/↓ scroll · v select · 1-6 highlight · n note · [ {previous} · ] {next} · Tab menu · q quit"
            )
        }
    });
    frame.render_widget(
        Paragraph::new(TuiLine::from(vec![
            TuiSpan::styled(status, Style::default().fg(Color::DarkGray)),
            TuiSpan::raw("  "),
            TuiSpan::styled(hint, Style::default().fg(Color::Gray)),
        ])),
        area,
    );
}

/// Render the reader's viewport: cursorline, visual selection, token cursor
/// and (in link mode) the row the focused link lives on.
fn reader_lines(
    reader: &Reader,
    theme: &Theme,
    height: usize,
    marked: Option<usize>,
) -> Vec<TuiLine<'static>> {
    let cursor = reader.cursor_line();
    let selecting = reader.selection.is_some();
    let overlays = reader.overlays();
    let glyph = if theme.glyphs == crate::render::Glyphs::Ascii {
        "> "
    } else {
        "▸ "
    };
    reader
        .viewport(height)
        .iter()
        .enumerate()
        .map(|(offset, line)| {
            let row = reader.scroll + offset;
            let mut rendered = ansi_to_tui_line(&line.text);
            // The link marker shifts the text right, so the overlay columns
            // have to follow it.
            let mut shift = 0usize;
            if marked == Some(row) {
                rendered.spans.insert(
                    0,
                    TuiSpan::styled(
                        glyph,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                );
                shift = glyph.chars().count();
            }
            let paint: Vec<(usize, usize, Modifier)> = overlays
                .iter()
                .filter(|(index, _)| *index == row)
                .flat_map(|(_, pieces)| {
                    pieces.iter().map(move |(start, end, kind)| {
                        (
                            start + shift,
                            end + shift,
                            overlay_modifier(*kind, selecting, theme.depth),
                        )
                    })
                })
                .collect();
            let selected = overlays
                .iter()
                .find(|(index, _)| *index == row)
                .is_some_and(|(_, pieces)| {
                    pieces
                        .iter()
                        .any(|(_, _, kind)| *kind == reader::OverlayKind::Selection)
                });
            // Colour mode paints the selection as a background band behind the
            // exact words a colour key would mark (below a highlight's own
            // colour, above the terminal default).
            let band = (selected && theme.depth != crate::render::ColorDepth::None)
                .then(|| overlay_band(theme.depth));
            let line_style = band.unwrap_or_default();
            let row_style = if row == cursor {
                cursor_line_style(theme)
            } else {
                Style::default()
            };
            let mut painted = paint_columns(rendered, &paint);
            // The row style only says "this line"; the indent indicator says
            // "these words", which is what a colour key would mark.
            painted
                .spans
                .insert(0, TuiSpan::raw(row_indicator(theme, selected, row == cursor)));
            // `Line::style` replaces the style, so merge the two layers by hand:
            // the cursor row wins (it spans the whole line), the selection band
            // shows through when it does not set a background.
            let merged = Style {
                bg: line_style.bg.or(row_style.bg),
                ..row_style
            };
            TuiLine::from(painted.spans).style(merged)
        })
        .collect()
}

/// Style of the row the cursor is on (nvim's `cursorline`).
fn cursor_line_style(theme: &Theme) -> Style {
    use crate::render::ColorDepth;
    if theme.depth == ColorDepth::None {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default().bg(Color::Indexed(236))
    }
}

/// How an overlay range is drawn.
///
/// The selection is the band the user is dragging; the cursor token is the
/// moving end of it. With no selection the cursor token takes the band, so the
/// word a colour would mark is always visible.
///
/// With colour the band is a background (reversed video would hide the
/// highlight colours underneath); without colour video is all we have, so the
/// selection is `REVERSED` and the moving end is underlined on top of it.
fn overlay_modifier(
    kind: reader::OverlayKind,
    selecting: bool,
    depth: crate::render::ColorDepth,
) -> Modifier {
    use crate::render::ColorDepth;
    match (kind, selecting, depth) {
        (reader::OverlayKind::Cursor, false, _) => Modifier::REVERSED,
        (reader::OverlayKind::Cursor, true, _) => Modifier::UNDERLINED | Modifier::BOLD,
        (reader::OverlayKind::Selection, _, ColorDepth::None) => Modifier::REVERSED,
        (reader::OverlayKind::Selection, _, _) => Modifier::empty(),
    }
}

/// Background band drawn behind the visual selection in colour mode.
fn overlay_band(depth: crate::render::ColorDepth) -> Style {
    use crate::render::ColorDepth;
    match depth {
        ColorDepth::None => Style::default(),
        _ => Style::default().bg(Color::Indexed(238)),
    }
}

/// Prefix indicators: nvim-style markers that say "this row is selected" and
/// "this row holds the cursor" even when video attributes are unreliable.
const SELECTION_MARKER: char = '│';
const CURSOR_MARKER: char = '▸';
const SELECTION_MARKER_ASCII: char = '|';
const CURSOR_MARKER_ASCII: char = '>';

/// The two prefix markers for the theme's glyph set.
fn markers(theme: &Theme) -> (char, char) {
    if theme.glyphs == crate::render::Glyphs::Ascii {
        (SELECTION_MARKER_ASCII, CURSOR_MARKER_ASCII)
    } else {
        (SELECTION_MARKER, CURSOR_MARKER)
    }
}

/// Two-column indicator for a row: selection flag, then cursor flag.
fn row_indicator(theme: &Theme, selected: bool, cursor: bool) -> String {
    let (selected_glyph, cursor_glyph) = markers(theme);
    match (selected, cursor) {
        (true, true) => format!("{selected_glyph}{cursor_glyph}"),
        (true, false) => format!("{selected_glyph} "),
        (false, true) => format!(" {cursor_glyph}"),
        (false, false) => "  ".to_string(),
    }
}

/// Add modifiers to the visible columns `[start, end)` of a parsed line.
///
/// Ranges compose, so the cursor token can be decorated on top of the selection
/// band. Column maths ignore ANSI escapes because the spans are already parsed.
///
/// The modifiers are baked into the spans: `Paragraph` patches the line style
/// *under* each span, so a band set on the line would be invisible on every
/// character that carries a colour of its own.
fn paint_columns(line: TuiLine<'static>, paint: &[(usize, usize, Modifier)]) -> TuiLine<'static> {
    if paint.is_empty() {
        return line;
    }
    let mut spans: Vec<TuiSpan<'static>> = Vec::new();
    let mut column = 0usize;
    for span in line.spans {
        let mut buffer = String::new();
        let mut current = span.style;
        for character in span.content.chars() {
            let style = paint
                .iter()
                .filter(|(start, end, _)| column >= *start && column < *end)
                .fold(span.style, |style, (_, _, added)| style.add_modifier(*added));
            if style != current && !buffer.is_empty() {
                spans.push(TuiSpan::styled(std::mem::take(&mut buffer), current));
            }
            current = style;
            buffer.push(character);
            column += UnicodeWidthChar::width(character).unwrap_or(0);
        }
        if !buffer.is_empty() {
            spans.push(TuiSpan::styled(buffer, current));
        }
    }
    TuiLine::from(spans)
}

/// Helper: render viewport lines for the current reader.
fn viewport_lines(app: &App, height: usize, marked: Option<usize>) -> Vec<TuiLine<'static>> {
    reader_lines(&app.reader, &app.theme, height, marked)
}

fn draw_reader(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let viewport = usize::from(area.height.saturating_sub(2));
    let marked = app
        .link_focus
        .then(|| app.reader.links().get(app.link_index))
        .flatten()
        .and_then(|link| app.reader.line_of_node(link.node));
    let lines = viewport_lines(app, viewport, marked);
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

fn draw_site_search(frame: &mut ratatui::Frame<'_>, area: Rect, buffer: &str, app: &App) {
    let popup = Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(3),
        width: area.width,
        height: 3,
    };
    frame.render_widget(Clear, popup);
    let title = app
        .home
        .as_ref()
        .and_then(|home| home.chrome.search.as_ref())
        .map(|search| format!(" {} ", search.placeholder))
        .unwrap_or_else(|| " search WOL ".to_string());
    frame.render_widget(
        Paragraph::new(format!("{buffer}\u{258f}"))
            .block(Block::default().borders(Borders::ALL).title(title)),
        popup,
    );
}

fn draw_settings(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, row: usize) {
    let popup = centered(area, 66, 46);
    frame.render_widget(Clear, popup);
    let settings = app.context.state.get_settings().unwrap_or_default();
    let language = app
        .home
        .as_ref()
        .and_then(|home| home.chrome.language.as_ref())
        .map(|entry| entry.label.clone())
        .filter(|label| !label.trim().is_empty())
        .unwrap_or_else(|| settings.language.clone());
    let home_page = if settings.home.trim().is_empty() {
        home_target(app)
    } else {
        settings.home.clone()
    };

    let entries = [
        ("Language", language),
        ("Home page", home_page),
        ("Close", String::new()),
    ];
    let mut lines: Vec<TuiLine<'_>> = Vec::new();
    for (index, (label, value)) in entries.iter().enumerate() {
        let selected = index == row;
        let style = if selected {
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        lines.push(TuiLine::from(TuiSpan::styled(
            format!(" {label:<12} {value} "),
            style,
        )));
    }
    lines.push(TuiLine::from(""));
    lines.push(TuiLine::from(TuiSpan::styled(
        format!(" data dir   {}", app.context.data_dir.display()),
        Style::default().fg(Color::DarkGray),
    )));
    if let Some((count, bytes)) = app.settings_stats {
        lines.push(TuiLine::from(TuiSpan::styled(
            format!(" cache      {count} pages · {}", human_bytes(bytes)),
            Style::default().fg(Color::DarkGray),
        )));
    }
    lines.push(TuiLine::from(""));
    lines.push(TuiLine::from(TuiSpan::styled(
        "↑/↓ move · Enter select · Esc close",
        Style::default().fg(Color::Gray),
    )));
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title(" settings "))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Shorten `text` to `width` terminal columns, appending `…` when clipped.
fn clip(text: &str, width: usize) -> String {
    use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
    if width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(text) <= width {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for character in text.chars() {
        let columns = UnicodeWidthChar::width(character).unwrap_or(0);
        if used + columns + 1 > width {
            break;
        }
        out.push(character);
        used += columns;
    }
    out.push('…');
    out
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
    fn home_pages_open_the_dashboard_with_todays_daily_text() {
        let html = r##"
        <html><body>
          <div id="siteBanner"><div class="title"><a href="/es/wol/h/r4/lp-s">
            <span class="titleBig">BIBLIOTECA EN LÍNEA Watchtower</span></a></div>
            <div class="language"><a href="/es/wol/li/r4/lp-s"><span class="label">español</span></a></div></div>
          <form class="searchForm" action="/es/wol/qt/r4/lp-s">
            <input class="searchField" placeholder="Introducir búsqueda"></form>
          <ul id="menuBar">
            <li id="menuBible"><a href="/es/wol/binav/r4/lp-s"><span class="label">BIBLIA</span></a></li>
            <li id="menuPublications"><a href="/es/wol/library/r4/lp-s/biblioteca"><span class="label">PUBLICACIONES</span></a></li>
            <li id="menuToday"><a href="/es/wol/meetings/r4/lp-s"><span class="label">REUNIONES</span></a></li>
          </ul>
          <div id="dailyText">
            <div id="todayNav">
              <div id="footerPrevDay"><a href="/es/wol/h/r4/lp-s/2026/11/15" aria-label="día anterior"></a></div>
              <div id="navigationDailyTextToday"><a href="/es/wol/h/r4/lp-s"><span class="navContent">Hoy</span></a></div>
              <div id="footerNextDay"><a href="/es/wol/h/r4/lp-s/2026/11/17" aria-label="día siguiente"></a></div>
            </div>
            <div class="tabContent" data-date="2026-11-15T00:00:00.000Z"><h2 data-pid="44">Domingo</h2></div>
            <div class="tabContent" data-date="2026-11-16T00:00:00.000Z"><h2 data-pid="47">Lunes</h2></div>
            <div class="tabContent" data-date="2026-11-17T00:00:00.000Z"><h2 data-pid="50">Martes</h2></div>
          </div>
        </body></html>
        "##;
        let page = build_home(html, "https://wol.jw.org/es/wol/h/r4/lp-s/2026/11/16")
            .expect("dated home page becomes a dashboard");
        assert_eq!(page.home.buttons.len(), 4);
        assert_eq!(page.home.pane_title(), " Lunes ");
        assert_eq!(
            page.daily.block("47").map(|block| block.text.as_str()),
            Some("Lunes"),
            "the requested day, not the first section, is shown"
        );
        assert_eq!(
            page.home.next.as_deref(),
            Some("/es/wol/h/r4/lp-s/2026/11/17")
        );

        // Ordinary documents and pages without a toolbar stay in the reader.
        assert!(build_home(
            "<article><p data-pid=\"1\">Hi</p></article>",
            "https://wol.jw.org/es/wol/d/r4/lp-s/1"
        )
        .is_none());
    }

    #[test]
    fn directory_pages_are_labelled_from_the_site_menu() {
        let html = r##"
        <html><body>
          <ul id="menuBar">
            <li id="menuBible"><a href="/es/wol/binav/r4/lp-s"><span class="label">BIBLIA</span></a></li>
            <li id="menuPublications"><a href="/es/wol/library/r4/lp-s/biblioteca"><span class="label">PUBLICACIONES</span></a></li>
            <li id="menuToday"><a href="/es/wol/meetings/r4/lp-s/2026/47"><span class="label">REUNIONES</span></a></li>
          </ul>
          <article id="article">
            <div id="materialNav"><ul class="directory">
              <li><a href="/es/wol/d/r4/lp-s/1"><div class="cardLine1">Uno</div></a></li>
            </ul></div>
          </article>
        </body></html>
        "##;
        let chrome = parse_chrome(html);
        assert_eq!(
            channel_label(&chrome, "https://wol.jw.org/es/wol/meetings/r4/lp-s").as_deref(),
            Some("REUNIONES")
        );
        assert_eq!(
            channel_label(&chrome, "https://wol.jw.org/es/wol/library/r4/lp-s/biblioteca").as_deref(),
            Some("PUBLICACIONES")
        );
        assert_eq!(
            channel_label(&chrome, "https://wol.jw.org/es/wol/binav/r4/lp-s").as_deref(),
            Some("BIBLIA")
        );
        assert_eq!(channel_label(&chrome, "https://wol.jw.org/es/wol/d/r4/lp-s/1"), None);
        assert_eq!(section_key("/es/wol/meetings/r4/lp-s/2026/47").as_deref(), Some("meetings"));
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

    #[test]
    fn paint_columns_marks_only_the_painted_range() {
        let painted = paint_columns(
            ansi_to_tui_line("   2 │ Jehovah God invites"),
            &[(7, 14, Modifier::REVERSED)],
        );
        assert_eq!(
            styles(&painted),
            vec![
                ("   2 │ ".to_string(), Style::default()),
                (
                    "Jehovah".to_string(),
                    Style::default().add_modifier(Modifier::REVERSED)
                ),
                (" God invites".to_string(), Style::default()),
            ]
        );
    }

    #[test]
    fn paint_columns_keeps_existing_colour_and_layers_the_cursor() {
        // A highlight keeps its background, and the cursor token adds modifiers
        // on top of the selection band.
        let painted = paint_columns(
            ansi_to_tui_line("\x1b[48;2;255;227;107mJehovah\x1b[0m God"),
            &[(0, 7, Modifier::REVERSED), (0, 7, Modifier::UNDERLINED)],
        );
        let styled = styles(&painted);
        assert_eq!(styled[0].0, "Jehovah");
        assert_eq!(
            styled[0].1,
            Style::default()
                .bg(Color::Rgb(255, 227, 107))
                .add_modifier(Modifier::REVERSED | Modifier::UNDERLINED)
        );
        assert_eq!(styled[1].1, Style::default());
    }

    const READER_HTML: &str = r##"
<div id="article">
  <p data-pid="1">How Can You Draw Close to God?</p>
  <p data-pid="2">Jehovah God invites all people to draw close to him.</p>
</div>
"##;

    fn fixture_reader() -> Reader {
        let mut document = wol_core::dom::parse_document(READER_HTML);
        document.source_url = Some("https://wol.jw.org/en/wol/d/r1/lp-e/2025441".to_string());
        Reader::new(
            document,
            wol_core::backup::StoredIndex::default(),
            &Theme::new(ColorDepth::TrueColor, Glyphs::Unicode),
            RenderOptions {
                width: 60,
                header: false,
                show_notes: false,
                show_footnotes: false,
                ..RenderOptions::default()
            },
        )
    }

    fn theme(depth: ColorDepth) -> Theme {
        Theme::new(depth, Glyphs::Unicode)
    }

    /// Every line of the viewport as plain text (styles dropped).
    fn texts(reader: &Reader, theme: &Theme) -> Vec<String> {
        reader_lines(reader, theme, 8, None)
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.to_string())
                    .collect::<String>()
            })
            .collect()
    }

    /// Text drawn with a modifier, grouped per rendered line.
    fn marked_lines(reader: &Reader, theme: &Theme, modifier: Modifier) -> Vec<String> {
        reader_lines(reader, theme, 8, None)
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .filter(|span| span.style.add_modifier.contains(modifier))
                    .map(|span| span.content.to_string())
                    .collect::<String>()
            })
            .filter(|text: &String| !text.is_empty())
            .collect()
    }

    /// Whole-word view of a line, so tests do not depend on where spans split.
    fn words_under(reader: &Reader, theme: &Theme, modifier: Modifier) -> Vec<String> {
        marked_lines(reader, theme, modifier)
            .iter()
            .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect()
    }

    fn selecting_reader() -> Reader {
        let mut reader = fixture_reader();
        reader.jump_to("2");
        reader.cursor_token = 6;
        reader.begin_selection();
        reader.move_cursor_token(1);
        reader
    }

    #[test]
    fn the_token_cursor_is_visible_without_a_selection() {
        let mut reader = fixture_reader();
        reader.jump_to("2");
        reader.cursor_token = 1;
        assert_eq!(
            words_under(&reader, &theme(ColorDepth::TrueColor), Modifier::REVERSED),
            vec!["God".to_string()]
        );
        // The gutter marks the row too, so a terminal that ignores video still
        // shows where the cursor is.
        assert!(texts(&reader, &theme(ColorDepth::None))
            .iter()
            .any(|line| line.starts_with(" ▸")));
    }



    #[test]
    fn the_visual_selection_is_visible_before_a_colour_is_applied() {
        let reader = selecting_reader();
        let colour = theme(ColorDepth::TrueColor);

        // The band covers exactly the words a colour key would mark.
        let lines = reader_lines(&reader, &colour, 8, None);

        let band = lines
            .iter()
            .filter(|line| line.style.bg == Some(Color::Indexed(238)))
            .count();
        assert_eq!(band, 1, "one row carries the selection band");

        // The moving end of the selection is underlined.
        assert_eq!(
            words_under(&reader, &colour, Modifier::UNDERLINED),
            vec!["close".to_string()]
        );
        // And the gutter says the row is selected.
        assert!(texts(&reader, &theme(ColorDepth::None))
            .iter()
            .any(|line| line.starts_with("│▸")));
    }

    #[test]
    fn without_colour_the_selection_band_is_reversed() {
        let reader = selecting_reader();
        let mono = theme(ColorDepth::None);
        assert_eq!(
            words_under(&reader, &mono, Modifier::REVERSED),
            vec!["draw close".to_string()]
        );
    }

}
