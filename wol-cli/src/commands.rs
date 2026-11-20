//! Implementation of every `wol` subcommand.

use std::collections::BTreeSet;
use std::io::{IsTerminal as _, Write};
use std::path::Path;

use wol_core::backup::{self, StoredIndex};
use wol_core::dom::parse_document;
use wol_core::sync::SyncManager;

use crate::context::Context;
use crate::render::article::{render_document, RenderOptions};
use crate::render::table::Table;
use crate::{CacheCommand, Cli, Command, ExportFormat, SyncCommand};

pub async fn run(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let context = match cli.data_dir.clone() {
        Some(dir) => Context::open_in(dir)?,
        None => Context::open()?,
    };

    let command = cli.command.clone().unwrap_or(Command::Tui { target: None });

    match &command {
        Command::Read {
            target,
            refresh,
            no_gutter,
            no_notes,
            legend,
            links,
            output,
        } => {
            read(
                cli,
                &context,
                target,
                *refresh,
                !*no_gutter,
                !*no_notes,
                *legend,
                *links,
                output.as_deref(),
            )
            .await
        }
        Command::Import {
            file,
            recount,
            merge,
        } => import(cli, &context, file, *recount, *merge),
        Command::Export {
            out,
            format,
            meps_lang,
        } => export(cli, &context, out, *format, *meps_lang),
        Command::Cache { command } => cache(cli, &context, command).await,
        Command::Sync { command } => sync(cli, &context, command).await,
        Command::Tui { target } => crate::tui::run(cli, &context, target.as_deref()).await,
        Command::Info => info(cli, &context),
    }
}

fn theme(cli: &Cli) -> crate::render::Theme {
    crate::theme_for(cli)
}

fn print_lines(_cli: &Cli, lines: &[String]) -> std::io::Result<()> {
    let text = format!("{}\n", lines.join("\n"));
    if std::io::stdout().is_terminal() && std::env::var_os("WOL_NO_PAGER").is_none() {
        let pager = std::env::var("PAGER").unwrap_or_else(|_| "less".to_string());
        let mut parts = pager.split_whitespace();
        if let Some(program) = parts.next() {
            let mut command = std::process::Command::new(program);
            command.args(parts);
            // `less -R` keeps highlight colours; harmless for other pagers.
            command.args(["-R", "-F", "-X"]);
            command.stdin(std::process::Stdio::piped());
            if let Ok(mut child) = command.spawn() {
                if let Some(stdin) = child.stdin.as_mut() {
                    let _ = stdin.write_all(text.as_bytes());
                }
                let _ = child.wait();
                return Ok(());
            }
        }
    }
    let mut stdout = std::io::stdout();
    stdout.write_all(text.as_bytes())?;
    stdout.flush()
}

fn marks_for(context: &Context) -> StoredIndex {
    context
        .state
        .get_stored_index()
        .ok()
        .flatten()
        .unwrap_or_default()
}

#[allow(clippy::too_many_arguments)]
async fn read(
    cli: &Cli,
    context: &Context,
    target: &str,
    refresh: bool,
    gutter: bool,
    notes: bool,
    legend: bool,
    links: bool,
    output: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let theme = theme(cli);
    let page = if refresh {
        let canonical = context.canonicalize(target)?;
        context.fetch(canonical).await?
    } else {
        context.load(target, cli.offline).await?
    };

    let mut document = parse_document(&page.html);
    document.source_url = Some(page.canonical.as_str().to_string());
    if document.nodes.is_empty() {
        return Err(format!(
            "{} does not look like a WOL article (no readable paragraphs found)",
            page.canonical
        )
        .into());
    }
    let marks = marks_for(context);
    let options = RenderOptions {
        width: crate::width_for(cli),
        gutter,
        show_notes: notes,
        show_footnotes: true,
        header: true,
    };
    let mut lines = render_document(&document, &marks, &theme, options);
    lines.push(String::new());
    lines.push(format!(
        "{} · {}",
        theme.paint(crate::render::StyleRole::Dim, page.source.label()),
        theme.paint(crate::render::StyleRole::Dim, page.canonical.as_str())
    ));
    if links {
        lines.push(String::new());
        lines.push(theme.paint(
            crate::render::StyleRole::Accent,
            &format!("Links ({})", document.links.len()),
        ));
        for (index, link) in document.links.iter().enumerate() {
            let label: String = link.text.trim().chars().take(60).collect();
            lines.push(format!(
                "  {:>3}. {}{}",
                index + 1,
                theme.paint(crate::render::StyleRole::Body, &label),
                theme.paint(crate::render::StyleRole::Dim, &format!("  {}", link.href)),
            ));
        }
    }
    if legend {
        lines.push(String::new());
        lines.push(theme.paint(crate::render::StyleRole::Accent, "Highlight colours"));
        for (index, entry) in crate::render::highlight::legend(&theme).into_iter().enumerate() {
            let _ = index;
            lines.push(format!("  {entry}"));
        }
    }

    match output {
        Some(path) => {
            std::fs::write(path, format!("{}\n", lines.join("\n")))?;
            eprintln!("wol: wrote {}", path.display());
            Ok(())
        }
        None => Ok(print_lines(cli, &lines)?),
    }
}

fn import(
    cli: &Cli,
    context: &Context,
    file: &Path,
    recount: bool,
    merge: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let theme = theme(cli);
    let mut index = backup::import_backup(file)?;
    if recount {
        index = backup::recount(&index);
    }
    if merge {
        let existing = marks_for(context);
        index = merge_indices(&existing, &index);
    }
    context.state.set_stored_index(&index)?;

    let mut table = Table::new(["metric", "value"]).max_width(crate::width_for(cli));
    table.push_row(crate::row!["file", file.display().to_string()]);
    table.push_row(crate::row!["highlights", index.highlight_count.to_string()]);
    table.push_row(crate::row!["notes + inputs", index.note_count.to_string()]);
    table.push_row(crate::row!["documents", index.document_count.to_string()]);
    table.push_row(crate::row!["bible chapters", index.bible_chapter_count.to_string()]);
    table.push_row(crate::row!["ignored rows", index.ignored.to_string()]);
    table.push_row(crate::row!["mode", if merge { "merged".into() } else { "replaced".to_string() }]);
    for line in table.render(&theme) {
        println!("{line}");
    }
    Ok(())
}

/// Union two indexes: highlights are merged per block, notes are appended.
fn merge_indices(existing: &StoredIndex, incoming: &StoredIndex) -> StoredIndex {
    let mut merged = incoming.clone();
    for (document_id, paragraphs) in &existing.docs {
        let target = merged.docs.entry(document_id.clone()).or_default();
        for (pid, spans) in paragraphs {
            let list = target.entry(pid.clone()).or_default();
            for span in spans {
                if !list.contains(span) {
                    list.push(*span);
                }
            }
            list.sort_by_key(|(start, end, _)| (*start, *end));
        }
    }
    for (document_id, meta) in &existing.doc_meta {
        merged
            .doc_meta
            .entry(document_id.clone())
            .or_insert_with(|| meta.clone());
    }
    for (key, verses) in &existing.bible {
        let target = merged.bible.entry(key.clone()).or_default();
        for (verse, spans) in verses {
            let list = target.entry(verse.clone()).or_default();
            for span in spans {
                if !list.contains(span) {
                    list.push(*span);
                }
            }
            list.sort_by_key(|(start, end, _)| (*start, *end));
        }
    }
    for (document_id, paragraphs) in &existing.notes.docs {
        let target = merged.notes.docs.entry(document_id.clone()).or_default();
        for (pid, notes) in paragraphs {
            let list = target.entry(pid.clone()).or_default();
            for note in notes {
                if !list.iter().any(|other| other.guid == note.guid && note.guid.is_some()) {
                    list.push(note.clone());
                }
            }
        }
    }
    for (document_id, fields) in &existing.input_fields.docs {
        let target = merged.input_fields.docs.entry(document_id.clone()).or_default();
        for (tag, value) in fields {
            target.entry(tag.clone()).or_insert_with(|| value.clone());
        }
    }
    merged.enabled = existing.enabled && incoming.enabled || existing.enabled;
    merged.file_name = existing
        .file_name
        .clone()
        .or_else(|| incoming.file_name.clone());
    merged.loaded_at = incoming
        .loaded_at
        .clone()
        .or_else(|| existing.loaded_at.clone());
    backup::recount(&merged)
}

fn export(
    cli: &Cli,
    context: &Context,
    out: &Path,
    format: Option<ExportFormat>,
    meps_lang: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    let index = marks_for(context);
    if index.highlight_count == 0 && index.note_count == 0 {
        eprintln!("wol: no highlights imported yet — run `wol import <file.jwlibrary>` first");
    }
    let format = format.unwrap_or_else(|| {
        let extension = out
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if extension == "jwlibrary" {
            ExportFormat::Jwlibrary
        } else {
            ExportFormat::Json
        }
    });
    match format {
        ExportFormat::Json => {
            let value = serde_json::to_value(&index)?;
            std::fs::write(out, serde_json::to_vec_pretty(&value)?)?;
        }
        ExportFormat::Jwlibrary => {
            let value = serde_json::to_value(&index)?;
            wol_core::jwlibrary_export::export_marks_to_jwlibrary(&value, out, meps_lang)
                .map_err(|err| format!("export failed: {err}"))?;
        }
    }
    if !cli.offline {
        // keep the shared state in sync with what we just wrote
        let _ = context.state.refresh_stored_index();
    }
    println!("wol: exported to {}", out.display());
    Ok(())
}

async fn cache(
    cli: &Cli,
    context: &Context,
    command: &CacheCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    let theme = theme(cli);
    let width = crate::width_for(cli);
    match command {
        CacheCommand::Status => {
            let total = context.cache.total_size()?;
            let count = context.cache.entry_count()?;
            let cap = context.cache.cap_bytes();
            let mut table = Table::new(["property", "value"]).max_width(width);
            table.push_row(crate::row!["data dir", context.data_dir.display().to_string()]);
            table.push_row(crate::row!["entries", count.to_string()]);
            table.push_row(crate::row!["size", format_size(total)]);
            table.push_row(crate::row!["cap", if cap == 0 { "unlimited".into() } else { format_size(cap) }]);
            table.push_row(crate::row![
                "used",
                format!("{:.1}%", if cap == 0 { 0.0 } else { total as f64 * 100.0 / cap as f64 }),
            ]);
            for line in table.render(&theme) {
                println!("{line}");
            }
        }
        CacheCommand::List { limit, filter } => {
            let entries = context
                .cache
                .list_entries(*limit, 0, filter.as_deref())
                .map_err(|err| err.to_string())?;
            let mut table = Table::new(["size", "type", "last used", "url"]).max_width(width);
            for entry in &entries {
                table.push_row(crate::row![
                    format_size(entry.size),
                    short_type(&entry.content_type),
                    age_label(entry.last_used_at),
                    entry.url.as_str().to_string(),
                ]);
            }
            if table.is_empty() {
                println!("{}", theme.paint(crate::render::StyleRole::Dim, "cache is empty"));
                return Ok(());
            }
            for line in table.render(&theme) {
                println!("{line}");
            }
            let total = usize::try_from(context.cache.entry_count().unwrap_or(0)).unwrap_or(0);
            if table.len() < total {
                println!(
                    "{}",
                    theme.paint(
                        crate::render::StyleRole::Dim,
                        &format!("{} of {total} entries — raise --limit to list more", table.len()),
                    )
                );
            }
        }
        CacheCommand::Crawl {
            target,
            follow,
            max_pages,
        } => {
            let page = context.fetch(context.canonicalize(target)?).await?;
            let mut crawled = 1usize;
            let subresources = subresource_urls(&page.html, &context.base_host());
            for url in &subresources {
                match context.canonicalize(url) {
                    Ok(canonical) => {
                        if context.cache.is_cached(&canonical).unwrap_or(false) {
                            continue;
                        }
                        if context.fetch_and_cache(&canonical).await.is_ok() {
                            crawled += 1;
                        }
                    }
                    Err(_) => continue,
                }
            }
            let mut pages = 1usize;
            if *follow {
                for link in internal_links(&page.html) {
                    if pages >= *max_pages {
                        break;
                    }
                    if let Ok(canonical) = context.canonicalize(&link) {
                        if context.cache.is_cached(&canonical).unwrap_or(false) {
                            continue;
                        }
                        if context.fetch(canonical).await.is_ok() {
                            pages += 1;
                        }
                    }
                }
            }
            println!(
                "wol: cached {crawled} resources across {pages} page(s) from {}",
                page.canonical
            );
        }
        CacheCommand::Clear => {
            context.cache.clear_all()?;
            println!("wol: cache cleared");
        }
        CacheCommand::Prune => {
            let before = context.cache.total_size()?;
            if !cli.offline {
                // Pruning is local only; never fetch while pruning.
            }
            context.cache.prune_to_cap()?;
            let after = context.cache.total_size()?;
            println!(
                "wol: pruned {} ({} -> {})",
                format_size(before.saturating_sub(after)),
                format_size(before),
                format_size(after)
            );
        }
    }
    Ok(())
}

fn subresource_urls(html: &str, base_host: &str) -> Vec<String> {
    let Ok(regex) = regex::Regex::new(r#"(?:src|href)=["']([^"']+)["']"#) else {
        return Vec::new();
    };
    let mut seen = BTreeSet::new();
    for capture in regex.captures_iter(html) {
        let raw = &capture[1];
        if raw.starts_with("/assets/")
            || raw.starts_with("/icon/")
            || raw.starts_with("/img/")
            || raw.starts_with("wol://")
            || raw.starts_with("https://") && raw.contains("/assets/")
        {
            seen.insert(raw.to_string());
        } else if raw.starts_with('/') && raw.contains("/wol/mp/") {
            seen.insert(format!("https://{base_host}{raw}"));
        }
    }
    seen.into_iter().collect()
}

fn internal_links(html: &str) -> Vec<String> {
    let Ok(regex) = regex::Regex::new(r#"href=["']([^"']*?/wol/d/[^"']+)["']"#) else {
        return Vec::new();
    };
    let mut seen = BTreeSet::new();
    for capture in regex.captures_iter(html) {
        seen.insert(capture[1].to_string());
    }
    seen.into_iter().collect()
}

async fn sync(
    cli: &Cli,
    context: &Context,
    command: &SyncCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    let theme = theme(cli);
    let manager = SyncManager::new_shared(context.state.clone());
    match command {
        SyncCommand::Status => {
            let status = manager.status()?;
            let mut table = Table::new(["property", "value"]).max_width(crate::width_for(cli));
            table.push_row(crate::row!["provider", format!("{:?}", status.provider)]);
            table.push_row(crate::row!["connected", status.connected.to_string()]);
            table.push_row(crate::row![
                "account",
                status.account_label.clone().unwrap_or_else(|| "-".into()),
            ]);
            table.push_row(crate::row![
                "server",
                status.server_url.clone().unwrap_or_else(|| "-".into()),
            ]);
            table.push_row(crate::row![
                "remote",
                status.remote_path.clone().unwrap_or_else(|| "-".into()),
            ]);
            table.push_row(crate::row![
                "last sync",
                status.last_synced_at.clone().unwrap_or_else(|| "never".into()),
            ]);
            table.push_row(crate::row![
                "local change",
                status.local_updated_at.clone().unwrap_or_else(|| "-".into()),
            ]);
            table.push_row(crate::row!["pending", status.changed_local.to_string()]);
            table.push_row(crate::row!["error", status.last_error.clone().unwrap_or_else(|| "-".into())]);
            for line in table.render(&theme) {
                println!("{line}");
            }
        }
        SyncCommand::Pull | SyncCommand::Push => {
            if cli.offline {
                return Err("sync needs network access; drop --offline".into());
            }
            let status = manager
                .sync_now()
                .await
                .map_err(|err| format!("sync failed: {err}"))?;
            println!(
                "wol: synced ({} provider, changed local: {})",
                format!("{:?}", status.provider),
                status.changed_local
            );
        }
    }
    Ok(())
}

fn info(cli: &Cli, context: &Context) -> Result<(), Box<dyn std::error::Error>> {
    let theme = theme(cli);
    let settings = context.state.get_settings().unwrap_or_default();
    let marks = marks_for(context);
    let mut table = Table::new(["property", "value"]).max_width(crate::width_for(cli));
    table.push_row(crate::row!["data dir", context.data_dir.display().to_string()]);
    table.push_row(crate::row!["base host", context.base_host()]);
    table.push_row(crate::row!["language", settings.language.clone()]);
    table.push_row(crate::row!["region", settings.region.to_string()]);
    table.push_row(crate::row!["cache entries", context.cache.entry_count()?.to_string()]);
    table.push_row(crate::row!["cache size", format_size(context.cache.total_size()?)]);
    table.push_row(crate::row!["highlights loaded", marks.highlight_count.to_string()]);
    table.push_row(crate::row!["marks file", marks.file_name.clone().unwrap_or_else(|| "-".into())]);
    table.push_row(crate::row![
        "colour",
        format!("{:?}", crate::theme_for(cli).depth),
    ]);
    for line in table.render(&theme) {
        println!("{line}");
    }
    Ok(())
}

fn short_type(content_type: &str) -> String {
    content_type
        .split(';')
        .next()
        .unwrap_or(content_type)
        .rsplit('/')
        .next()
        .unwrap_or(content_type)
        .to_ascii_uppercase()
}

fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn age_label(epoch_secs: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let delta = (now - epoch_secs).max(0);
    if delta < 60 {
        "just now".into()
    } else if delta < 3_600 {
        format!("{}m ago", delta / 60)
    } else if delta < 86_400 {
        format!("{}h ago", delta / 3_600)
    } else {
        format!("{}d ago", delta / 86_400)
    }
}
