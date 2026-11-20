//! Home dashboard: WOL's own toolbar, welcome panel and today's daily text.
//!
//! The dashboard is not a hard-coded imitation of the desktop home page — the
//! buttons, labels, search endpoint and welcome copy are parsed out of the very
//! page the desktop renders (`wol_core::dom::parse_chrome` / `parse_welcome` /
//! `parse_daily_text`). That keeps it in sync with the site, in whatever
//! language the reader picked, and pointing at the same URLs.
//!
//! Everything here is pure: key handling only mutates `Home`, rendering lives in
//! `tui::mod`, so the selection logic is unit-testable.

use wol_core::dom::{Chrome, DailyText, Node_, Welcome};

/// What activating a toolbar button does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Open this site-relative or absolute URL in the reader.
    Open(String),
    /// Ask for a query, then open the site search.
    Search,
    /// Open the native settings sheet.
    Settings,
}

/// One toolbar button (`BIBLIA`, `PUBLICACIONES`, `REUNIONES`, search, gear).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    pub label: String,
    pub action: Action,
}

/// Which part of the dashboard receives the arrow keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// `←`/`→` move between buttons, `Enter` activates, `↓` enters the text.
    Menu,
    /// The daily-text pane: normal reader keys (scroll, select, highlight, note).
    Text,
}

/// Dashboard state built from one home page.
pub struct Home {
    pub chrome: Chrome,
    pub welcome: Option<Welcome>,
    /// `data-date` of the shown day, kept for messages.
    pub date: Option<String>,
    pub previous: Option<String>,
    pub next: Option<String>,
    pub label_previous: Option<String>,
    pub label_next: Option<String>,
    pub label_today: String,
    pub buttons: Vec<Button>,
    pub focus: Focus,
    pub menu_index: usize,
}

impl Home {
    pub fn new(chrome: Chrome, welcome: Option<Welcome>, daily: &DailyText) -> Self {
        let mut buttons: Vec<Button> = chrome
            .menu
            .iter()
            .map(|entry| Button {
                label: entry.label.clone(),
                action: Action::Open(entry.href.clone()),
            })
            .collect();
        if let Some(search) = &chrome.search {
            buttons.push(Button {
                label: search.placeholder.clone(),
                action: Action::Search,
            });
        }
        if let Some(preferences) = &chrome.preferences {
            buttons.push(Button {
                label: preferences.label.clone(),
                action: Action::Settings,
            });
        }
        Self {
            // The day's own heading (`Martes 17 de noviembre`) beats the generic
            // `Hoy` nav label as soon as the reader browses other days.
            label_today: daily_heading(daily)
                .or_else(|| daily.label_today.clone())
                .unwrap_or_else(|| "today".to_string()),
            date: daily.date.clone(),
            previous: daily.previous.clone(),
            next: daily.next.clone(),
            label_previous: daily.label_previous.clone(),
            label_next: daily.label_next.clone(),
            buttons,
            focus: Focus::Menu,
            menu_index: 0,
            chrome,
            welcome,
        }
    }

    /// Move the button selection, wrapping at both ends.
    pub fn step(&mut self, delta: isize) {
        let total = self.buttons.len() as isize;
        if total == 0 {
            self.menu_index = 0;
            return;
        }
        let next = (self.menu_index as isize + delta).rem_euclid(total);
        self.menu_index = next as usize;
    }

    pub fn selected(&self) -> Option<&Button> {
        self.buttons.get(self.menu_index)
    }

    /// Select a button by action kind (`Search`, `Settings`), if present.
    pub fn select_action(&mut self, kind: fn(&Action) -> bool) {
        if let Some(index) = self.buttons.iter().position(|button| kind(&button.action)) {
            self.menu_index = index;
        }
    }

    pub fn focus_text(&mut self) {
        self.focus = Focus::Text;
    }

    pub fn focus_menu(&mut self) {
        self.focus = Focus::Menu;
    }

    /// Title of the daily-text pane, in the page's language (`Hoy`).
    pub fn pane_title(&self) -> String {
        format!(" {} ", self.label_today)
    }

    /// URL of the site search form, or a settings-derived fallback.
    pub fn search_action(&self) -> Option<String> {
        self.chrome
            .search
            .as_ref()
            .map(|search| search.action.clone())
            .filter(|action| !action.trim().is_empty())
    }
}

/// First non-empty block of a daily-text section — its localised date heading.
fn daily_heading(daily: &DailyText) -> Option<String> {
    daily.document.nodes.iter().find_map(|node| match node {
        Node_::Block(block) => {
            let text = block.text.trim();
            (!text.is_empty()).then(|| text.chars().take(60).collect::<String>())
        }
        _ => None,
    })
}

/// The reader's local date as a `YYYY-MM-DD` prefix (matched against
/// `data-date`, which WOL encodes in the reader's own timezone).
///
/// `local-offset` can fail (unsound-to-detect environments); UTC is then a
/// reasonable fallback, and [`wol_core::dom::parse_daily_text`] still falls back
/// to the middle section when no prefix matches.
pub fn today_prefix() -> String {
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    format!("{:04}-{:02}-{:02}", now.year(), u8::from(now.month()), now.day())
}

/// Date of a dated daily-text page (`/es/wol/h/r4/lp-s/2026/11/15`).
///
/// A page explicitly asked for a day must show that day, not the reader's today:
/// WOL renders `yesterday/today/tomorrow` around whichever day the URL names.
pub fn daily_text_date(url: &str) -> Option<String> {
    let path = url.split(['?', '#']).next()?;
    let segments: Vec<&str> = path.split('/').filter(|segment| !segment.is_empty()).collect();
    if segments.len() < 3 {
        return None;
    }
    let [year, month, day] = [
        segments[segments.len() - 3],
        segments[segments.len() - 2],
        segments[segments.len() - 1],
    ];
    let shaped = year.len() == 4
        && month.len() == 2
        && day.len() == 2
        && year.chars().all(|c| c.is_ascii_digit())
        && month.chars().all(|c| c.is_ascii_digit())
        && day.chars().all(|c| c.is_ascii_digit());
    shaped.then(|| format!("{year}-{month}-{day}"))
}

/// `wol` search URL for `query`, honouring any query string already on `action`.
pub fn search_url(action: &str, query: &str) -> String {
    let separator = if action.contains('?') { '&' } else { '?' };
    format!(
        "{action}{separator}q={}&p=par&r=occ",
        encode_query(query.trim())
    )
}

/// Percent-encode a search query (`+` for spaces, UTF-8 for the rest).
pub fn encode_query(query: &str) -> String {
    let mut encoded = String::with_capacity(query.len());
    for byte in query.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(char::from(byte))
            }
            b' ' => encoded.push('+'),
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use wol_core::dom::parse_document;

    fn daily() -> DailyText {
        DailyText {
            date: Some("2026-11-16T00:00:00.000Z".into()),
            document: parse_document("<p data-pid=\"1\">Hoy</p>"),
            previous: Some("/es/wol/h/r4/lp-s/2026/11/15".into()),
            today: Some("/es/wol/h/r4/lp-s".into()),
            next: Some("/es/wol/h/r4/lp-s/2026/11/17".into()),
            label_previous: Some("día anterior".into()),
            label_today: Some("Hoy".into()),
            label_next: Some("día siguiente".into()),
        }
    }

    fn chrome() -> Chrome {
        Chrome {
            menu: vec![
                wol_core::dom::MenuEntry {
                    id: "menuBible".into(),
                    label: "BIBLIA".into(),
                    href: "/es/wol/binav/r4/lp-s".into(),
                },
                wol_core::dom::MenuEntry {
                    id: "menuPublications".into(),
                    label: "PUBLICACIONES".into(),
                    href: "/es/wol/library/r4/lp-s/biblioteca".into(),
                },
                wol_core::dom::MenuEntry {
                    id: "menuToday".into(),
                    label: "REUNIONES".into(),
                    href: "/es/wol/meetings/r4/lp-s".into(),
                },
            ],
            search: Some(wol_core::dom::SearchBox {
                action: "/es/wol/qt/r4/lp-s".into(),
                placeholder: "Introducir búsqueda".into(),
            }),
            preferences: Some(wol_core::dom::MenuEntry {
                id: "preferences".into(),
                label: "configuración".into(),
                href: "/es/wol/pref/r4/lp-s".into(),
            }),
            ..Chrome::default()
        }
    }

    #[test]
    fn buttons_follow_the_pages_own_toolbar() {
        let home = Home::new(chrome(), None, &daily());
        let labels: Vec<&str> = home.buttons.iter().map(|button| button.label.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "BIBLIA",
                "PUBLICACIONES",
                "REUNIONES",
                "Introducir búsqueda",
                "configuración"
            ]
        );
        assert_eq!(
            home.buttons[0].action,
            Action::Open("/es/wol/binav/r4/lp-s".into())
        );
        assert_eq!(home.buttons[3].action, Action::Search);
        assert_eq!(home.buttons[4].action, Action::Settings);
        assert_eq!(home.pane_title(), " Hoy ");
        assert_eq!(home.search_action().as_deref(), Some("/es/wol/qt/r4/lp-s"));
    }

    #[test]
    fn selection_wraps_in_both_directions() {
        let mut home = Home::new(chrome(), None, &daily());
        home.step(1);
        assert_eq!(home.selected().map(|button| button.label.as_str()), Some("PUBLICACIONES"));
        home.step(-2);
        assert_eq!(home.selected().map(|button| button.label.as_str()), Some("configuración"));
        home.step(1);
        assert_eq!(home.selected().map(|button| button.label.as_str()), Some("BIBLIA"));
        home.select_action(|action| matches!(action, Action::Search));
        assert_eq!(home.selected().map(|button| button.label.as_str()), Some("Introducir búsqueda"));
    }

    #[test]
    fn a_page_without_a_search_box_omits_the_button() {
        let mut chrome = chrome();
        chrome.search = None;
        chrome.preferences = None;
        let home = Home::new(chrome, None, &daily());
        assert_eq!(home.buttons.len(), 3);
        assert!(home.search_action().is_none());
    }

    #[test]
    fn dated_home_pages_yield_their_own_day() {
        assert_eq!(
            daily_text_date("https://wol.jw.org/es/wol/h/r4/lp-s/2026/11/15").as_deref(),
            Some("2026-11-15")
        );
        assert_eq!(
            daily_text_date("/en/wol/h/r1/lp-e/2026/1/5?#x").as_deref(),
            None,
            "month and day must be zero padded like WOL writes them"
        );
        assert_eq!(daily_text_date("/es/wol/h/r4/lp-s"), None);
        assert_eq!(daily_text_date(""), None);
    }

    #[test]
    fn search_urls_are_percent_encoded() {
        assert_eq!(
            search_url("/es/wol/qt/r4/lp-s", "amor de Jehová"),
            "/es/wol/qt/r4/lp-s?q=amor+de+Jehov%C3%A1&p=par&r=occ"
        );
        assert_eq!(
            search_url("/es/wol/s/r4/lp-s?x=1", "amor"),
            "/es/wol/s/r4/lp-s?x=1&q=amor&p=par&r=occ"
        );
        assert_eq!(encode_query("a&b=c?d"), "a%26b%3Dc%3Fd");
        assert_eq!(encode_query(""), "");
    }

    #[test]
    fn today_prefix_is_an_iso_day() {
        let prefix = today_prefix();
        assert_eq!(prefix.len(), 10);
        assert_eq!(&prefix[4..5], "-");
        assert_eq!(&prefix[7..8], "-");
        assert!(prefix.chars().filter(|c| c.is_ascii_digit()).count() >= 8);
    }
}
