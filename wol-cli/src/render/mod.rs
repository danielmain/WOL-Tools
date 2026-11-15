//! Theme & capability detection.
//!
//! Colour is opt-out, never guessed destructively: `NO_COLOR` and `--no-color`
//! win over everything, then `COLORTERM` selects 24-bit vs 256-colour, then a
//! plain terminal gets the ASCII-safe fallback palette.

use std::env;

pub mod article;
pub mod highlight;
pub mod table;

/// How much colour the terminal can render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorDepth {
    /// No ANSI at all (`--no-color`, `NO_COLOR`, dumb terminals, piped output).
    None,
    /// `\x1b[38;5;Nm` — 256-colour palette.
    Ansi256,
    /// `\x1b[38;2;R;G;Bm` — 24-bit TrueColor.
    TrueColor,
}

impl ColorDepth {
    /// Probe the environment the way `less`/`git` do.
    pub fn detect(force_no_color: bool) -> Self {
        if force_no_color || env::var_os("NO_COLOR").is_some() {
            return ColorDepth::None;
        }
        if env::var("TERM").is_ok_and(|term| term == "dumb") {
            return ColorDepth::None;
        }
        match env::var("COLORTERM").unwrap_or_default().to_ascii_lowercase().as_str() {
            "truecolor" | "24bit" => ColorDepth::TrueColor,
            _ => match env::var("TERM_PROGRAM").unwrap_or_default().as_str() {
                "iTerm.app" | "WezTerm" | "vscode" | "Apple_Terminal" => ColorDepth::TrueColor,
                _ => ColorDepth::Ansi256,
            },
        }
    }

    pub fn supports_color(self) -> bool {
        self != ColorDepth::None
    }

    pub fn supports_truecolor(self) -> bool {
        self == ColorDepth::TrueColor
    }
}

/// Border/rule character sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyphs {
    Unicode,
    Ascii,
}

impl Glyphs {
    pub fn detect(force_ascii: bool) -> Self {
        if force_ascii {
            return Glyphs::Ascii;
        }
        let lang = env::var("LC_ALL")
            .or_else(|_| env::var("LC_CTYPE"))
            .or_else(|_| env::var("LANG"))
            .unwrap_or_default()
            .to_ascii_lowercase();
        if lang.contains("utf-8") || lang.contains("utf8") {
            Glyphs::Unicode
        } else {
            Glyphs::Ascii
        }
    }
}

/// The 1–6 highlight palette, taken verbatim from `wol-highlights/content.css`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Nearest xterm-256 index (6×6×6 cube plus greyscale ramp).
    pub fn to_ansi256(self) -> u8 {
        const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];
        let index_of = |value: u8| -> u8 {
            CUBE.iter()
                .enumerate()
                .min_by_key(|(_, level)| level.abs_diff(value))
                .map(|(index, _)| index as u8)
                .unwrap_or(0)
        };
        16 + 36 * index_of(self.r) + 6 * index_of(self.g) + index_of(self.b)
    }
}

/// JW Library's highlight palette (`mark.wol-cN`), used for terminal highlights.
pub const HIGHLIGHT_COLORS: [Rgb; 7] = [
    Rgb::new(0xE6, 0xE6, 0xE6), // 0 — no colour / neutral
    Rgb::new(0xFF, 0xE3, 0x6B), // 1 — yellow
    Rgb::new(0xB6, 0xE8, 0x9A), // 2 — green
    Rgb::new(0xA9, 0xD4, 0xFB), // 3 — blue
    Rgb::new(0xF7, 0xB4, 0xC4), // 4 — pink
    Rgb::new(0xFF, 0xC4, 0x8A), // 5 — orange
    Rgb::new(0xD8, 0xC0, 0xF8), // 6 — purple
];

/// Human-readable names for the six highlight colours.
pub struct HighlightColor;

impl HighlightColor {
    pub fn label(color: i64) -> &'static str {
        match color {
            1 => "yellow",
            2 => "green",
            3 => "blue",
            4 => "pink",
            5 => "orange",
            6 => "purple",
            _ => "none",
        }
    }

}

pub fn highlight_rgb(color: i64) -> Rgb {
    let index = usize::try_from(color).ok().filter(|value| *value < HIGHLIGHT_COLORS.len());
    index.map(|value| HIGHLIGHT_COLORS[value]).unwrap_or(HIGHLIGHT_COLORS[1])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleRole {
    Title,
    Heading(u8),
    Body,
    Dim,
    Accent,
    Note,
    Caption,
    Link,
    Scripture,
    Highlight(i64),
}

/// Resolves roles into SGR sequences for the detected depth.
#[derive(Debug, Clone)]
pub struct Theme {
    pub depth: ColorDepth,
    pub glyphs: Glyphs,
}

impl Theme {
    pub fn new(depth: ColorDepth, glyphs: Glyphs) -> Self {
        Self { depth, glyphs }
    }

    pub fn plain() -> Self {
        Self::new(ColorDepth::None, Glyphs::Ascii)
    }

    fn sgr(&self, codes: &[String]) -> String {
        if self.depth == ColorDepth::None || codes.is_empty() {
            return String::new();
        }
        format!("\x1b[{}m", codes.join(";"))
    }

    fn fg(&self, value: Rgb) -> Option<String> {
        match self.depth {
            ColorDepth::None => None,
            ColorDepth::Ansi256 => Some(format!("38;5;{}", value.to_ansi256())),
            ColorDepth::TrueColor => Some(format!("38;2;{};{};{}", value.r, value.g, value.b)),
        }
    }

    fn bg(&self, value: Rgb) -> Option<String> {
        match self.depth {
            ColorDepth::None => None,
            ColorDepth::Ansi256 => Some(format!("48;5;{}", value.to_ansi256())),
            ColorDepth::TrueColor => Some(format!("48;2;{};{};{}", value.r, value.g, value.b)),
        }
    }

    /// Escape sequence that starts `role` (empty when colour is disabled).
    pub fn start(&self, role: StyleRole) -> String {
        let codes: Vec<String> = match role {
            StyleRole::Title => ["1".to_string(), self.fg(Rgb::new(0x9D, 0xC4, 0xFF)).unwrap_or_default()].to_vec(),
            StyleRole::Heading(level) => {
                let color = if level <= 1 { Rgb::new(0x9D, 0xC4, 0xFF) } else { Rgb::new(0x8F, 0xD6, 0xD0) };
                ["1".to_string(), self.fg(color).unwrap_or_default()].to_vec()
            }
            StyleRole::Body => self.fg(Rgb::new(0xD8, 0xDE, 0xE9)).into_iter().collect(),
            StyleRole::Dim => ["2".to_string(), self.fg(Rgb::new(0x7A, 0x84, 0x94)).unwrap_or_default()].to_vec(),
            StyleRole::Accent => self.fg(Rgb::new(0x8F, 0xD6, 0xD0)).into_iter().collect(),
            StyleRole::Note => self.fg(Rgb::new(0xB9, 0xD7, 0xFF)).into_iter().collect(),
            StyleRole::Caption => ["3".to_string(), self.fg(Rgb::new(0x94, 0x9C, 0xAA)).unwrap_or_default()].to_vec(),
            StyleRole::Link => ["4".to_string(), self.fg(Rgb::new(0x7F, 0xB2, 0xEB)).unwrap_or_default()].to_vec(),
            StyleRole::Scripture => self.fg(Rgb::new(0xC7, 0xB4, 0xEE)).into_iter().collect(),
            StyleRole::Highlight(color) => {
                // Dark text on the JW Library pastel; keeps contrast readable.
                let mut codes = Vec::new();
                if let Some(background) = self.bg(highlight_rgb(color)) {
                    codes.push(background);
                }
                if let Some(foreground) = self.fg(Rgb::new(0x10, 0x14, 0x1A)) {
                    codes.push(foreground);
                }
                codes
            }
        };
        let codes: Vec<String> = codes.into_iter().filter(|code| !code.is_empty()).collect();
        self.sgr(&codes)
    }

    pub const RESET: &'static str = "\x1b[0m";

    pub fn reset(&self) -> &'static str {
        if self.depth == ColorDepth::None {
            ""
        } else {
            Self::RESET
        }
    }

    /// Wrap `text` in `role`.
    pub fn paint(&self, role: StyleRole, text: &str) -> String {
        if self.depth == ColorDepth::None {
            return text.to_string();
        }
        let start = self.start(role);
        if start.is_empty() {
            return text.to_string();
        }
        format!("{start}{text}{}", self.reset())
    }

    /// Rounded frame corners, or ASCII fallbacks.
    pub fn border(&self) -> BorderSet {
        match self.glyphs {
            Glyphs::Unicode => BorderSet {
                top_left: '╭',
                top_right: '╮',
                bottom_left: '╰',
                bottom_right: '╯',
                horizontal: '─',
                vertical: '│',
                heavy_horizontal: '━',
                left_bar: '┃',
            },
            Glyphs::Ascii => BorderSet {
                top_left: '+',
                top_right: '+',
                bottom_left: '+',
                bottom_right: '+',
                horizontal: '-',
                vertical: '|',
                heavy_horizontal: '=',
                left_bar: '|',
            },
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::plain()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BorderSet {
    pub top_left: char,
    pub top_right: char,
    pub bottom_left: char,
    pub bottom_right: char,
    pub horizontal: char,
    pub vertical: char,
    pub heavy_horizontal: char,
    pub left_bar: char,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truecolor_background_and_dark_foreground() {
        let theme = Theme::new(ColorDepth::TrueColor, Glyphs::Unicode);
        let escape = theme.start(StyleRole::Highlight(1));
        assert!(escape.contains("48;2;255;227;107"));
        assert!(escape.contains("38;2;16;20;26"));
    }

    #[test]
    fn ansi256_approximates_the_palette() {
        assert_eq!(Rgb::new(0, 0, 0).to_ansi256(), 16);
        assert_eq!(Rgb::new(255, 255, 255).to_ansi256(), 231);
        let theme = Theme::new(ColorDepth::Ansi256, Glyphs::Unicode);
        assert!(theme.start(StyleRole::Highlight(2)).contains("48;5;"));
    }

    #[test]
    fn colourless_theme_emits_no_escapes() {
        let theme = Theme::plain();
        assert_eq!(theme.paint(StyleRole::Highlight(3), "text"), "text");
        assert_eq!(theme.start(StyleRole::Title), "");
        assert_eq!(theme.reset(), "");
    }

    #[test]
    fn ascii_borders_are_pure_ascii() {
        let borders = Theme::new(ColorDepth::None, Glyphs::Ascii).border();
        for glyph in [
            borders.top_left,
            borders.top_right,
            borders.bottom_left,
            borders.bottom_right,
            borders.horizontal,
            borders.vertical,
            borders.left_bar,
        ] {
            assert!(glyph.is_ascii(), "{glyph:?} is not ASCII");
        }
    }

    #[test]
    fn unknown_colours_fall_back_to_yellow() {
        assert_eq!(highlight_rgb(42), HIGHLIGHT_COLORS[1]);
        assert_eq!(highlight_rgb(0), HIGHLIGHT_COLORS[0]);
        assert_eq!(highlight_rgb(6), HIGHLIGHT_COLORS[6]);
    }
}
