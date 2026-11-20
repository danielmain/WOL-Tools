//! `wol` — pure-terminal OfflineWOL reader.
//!
//! One-shot commands print formatted articles/highlights to stdout (auto-paging
//! through `$PAGER` on a TTY); `wol tui` opens the full-screen reader. All state
//! (cache, `wolMarks`, sync) is shared with the desktop app via `wol-core`.

mod commands;
mod context;
mod render;
mod tui;

use std::io::IsTerminal as _;

use clap::{Parser, Subcommand};

use crate::render::{ColorDepth, Glyphs, Theme};

#[derive(Debug, Parser)]
#[command(
    name = "wol",
    version,
    about = "OfflineWOL in your terminal — cached wol.jw.org reading with JW Library highlights",
    long_about = "Browse Watchtower Online Library in the terminal: the library index, \
publications and articles, with the highlights and study notes from your .jwlibrary backup. \
Pages are served from the same cache OfflineWOL uses, so anything you browsed on the desktop \
is instantly readable. Run without a subcommand to open the home dashboard (Bible, \
publications, meetings, search and today's daily text, driven with the arrow keys) at your \
configured home page."
)]
pub struct Cli {
    /// Disable ANSI colour (also respects NO_COLOR).
    #[arg(long, global = true)]
    pub no_color: bool,

    /// Use ASCII-only borders and typography.
    #[arg(long, global = true)]
    pub ascii: bool,

    /// Output width in columns (defaults to the terminal width, capped at 120).
    #[arg(long, global = true, value_name = "COLUMNS")]
    pub width: Option<usize>,

    /// Never touch the network.
    #[arg(long, global = true)]
    pub offline: bool,

    /// Use an alternate data directory instead of the shared app data.
    #[arg(long, global = true, value_name = "DIR")]
    pub data_dir: Option<std::path::PathBuf>,

    /// Subcommand to run. With none, `wol` opens the interactive reader on the
    /// configured language home page (like launching OfflineWOL).
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    /// Fetch (or load from cache) a page and print it with highlights.
    Read {
        /// URL, `wol://` address, path, or bare document id.
        target: String,
        /// Force a network refresh before rendering.
        #[arg(long)]
        refresh: bool,
        /// Hide the paragraph gutter.
        #[arg(long)]
        no_gutter: bool,
        /// Hide study notes.
        #[arg(long)]
        no_notes: bool,
        /// Print the highlight colour legend after the article.
        #[arg(long)]
        legend: bool,
        /// List the page's links (targets for `wol tui`).
        #[arg(long)]
        links: bool,
        /// Save the rendered text to a file instead of stdout/$PAGER.
        #[arg(long, short, value_name = "FILE")]
        output: Option<std::path::PathBuf>,
    },

    /// Import highlights and notes from a .jwlibrary / userData.db / marks.json.
    Import {
        file: std::path::PathBuf,
        /// Recompute counters instead of copying them from the backup.
        #[arg(long)]
        recount: bool,
        /// Merge into the existing marks instead of replacing them.
        #[arg(long)]
        merge: bool,
    },

    /// Export highlights and notes as .jwlibrary or marks.json.
    Export {
        out: std::path::PathBuf,
        /// Written format (inferred from the extension when omitted).
        #[arg(long, value_enum)]
        format: Option<ExportFormat>,
        /// MEPS language id for Bible keys inside the .jwlibrary database.
        #[arg(long, default_value_t = 0)]
        meps_lang: i64,
    },

    /// Inspect and manage the offline cache.
    Cache {
        #[command(subcommand)]
        command: CacheCommand,
    },

    /// Run cloud sync headlessly.
    Sync {
        #[command(subcommand)]
        command: SyncCommand,
    },

    /// Open the full-screen interactive reader (default when no subcommand is given).
    /// A home page opens the dashboard: ←/→ pick a button, Enter opens, ↓/Tab reads the
    /// daily text. Directory pages open a two-pane browser (sub-categories left, content
    /// right) driven by j/k and Enter.
    Tui {
        /// Optional page to open immediately (defaults to the configured home page).
        target: Option<String>,
    },

    /// Print where state, cache and settings live.
    Info,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum ExportFormat {
    Jwlibrary,
    Json,
}

#[derive(Debug, Clone, Subcommand)]
pub enum CacheCommand {
    /// Show cache size, entry count and cap.
    Status,
    /// List cached entries, newest first.
    List {
        /// Maximum rows to print.
        #[arg(long, default_value_t = 40)]
        limit: usize,
        /// Only entries whose URL contains this text.
        #[arg(long)]
        filter: Option<String>,
    },
    /// Crawl a page and its subresources into the cache.
    Crawl {
        target: String,
        /// Follow internal article links one level deep.
        #[arg(long)]
        follow: bool,
        /// Maximum pages to fetch (with --follow).
        #[arg(long, default_value_t = 20)]
        max_pages: usize,
    },
    /// Remove every cached entry.
    Clear,
    /// Evict entries to fit the configured cap.
    Prune,
}

#[derive(Debug, Clone, Subcommand)]
pub enum SyncCommand {
    /// Show provider, connection state and last sync result.
    Status,
    /// Run a sync cycle (pull remote changes, then push local ones).
    Pull,
    /// Run a sync cycle (pull remote changes, then push local ones).
    Push,
}

/// Terminal capabilities resolved from flags + environment.
pub fn theme_for(cli: &Cli) -> Theme {
    let depth = ColorDepth::detect(cli.no_color || !std::io::stdout().is_terminal());
    let glyphs = Glyphs::detect(cli.ascii);
    Theme::new(depth, glyphs)
}

pub fn width_for(cli: &Cli) -> usize {
    cli.width
        .or_else(|| {
            std::io::stdout()
                .is_terminal()
                .then(terminal_size_columns)
                .flatten()
        })
        .unwrap_or(100)
        .clamp(20, 200)
}

fn terminal_size_columns() -> Option<usize> {
    crossterm::terminal::size().ok().map(|(columns, _)| usize::from(columns))
}

fn main() {
    let cli = Cli::parse();
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("wol: failed to start async runtime: {err}");
            std::process::exit(1);
        }
    };
    if let Err(err) = runtime.block_on(commands::run(&cli)) {
        eprintln!("wol: {err}");
        std::process::exit(1);
    }
}
