//! Guards the JS ↔ Tauri IPC contract for the manager webview.
//!
//! Tauri derives the IPC command name from the Rust function name, so a
//! mismatch (`save_imported_publication` in JS vs `save_imported_publication_command`
//! in Rust) is invisible to the compiler and to every other test: the UI just
//! rejects at runtime with "command not found". That is exactly how the JWPUB
//! import and the Publications list broke silently, so assert the contract here.
//!
//! Scope: literal `invoke("name")` / `tauriInvoke("name")` calls in
//! `wol-desktop/ui/**`. Dynamic invocations (a variable) cannot be checked
//! statically and are skipped.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

fn desktop_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// Command paths listed inside `tauri::generate_handler![…]`.
fn registered_commands() -> BTreeSet<String> {
    let lib_src = fs::read_to_string(desktop_dir().join("src/lib.rs")).expect("read src/lib.rs");
    let start = lib_src
        .find("generate_handler![")
        .expect("src/lib.rs must contain a generate_handler! block")
        + "generate_handler![".len();
    let body = &lib_src[start..];
    let end = body.find(']').expect("unterminated generate_handler! block");
    body[..end]
        .split(',')
        .filter_map(|entry| {
            let entry = entry.trim();
            let name = entry.rsplit("::").next()?.trim();
            (!name.is_empty()).then(|| name.to_string())
        })
        .collect()
}

fn ui_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            ui_files(&path, out);
        } else if matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("js" | "mjs" | "html")
        ) {
            out.push(path);
        }
    }
}

/// Literal command names passed to `invoke(...)`/`tauriInvoke(...)`.
fn invoked_commands(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    let bytes = source.as_bytes();
    for marker in ["invoke(", "tauriInvoke("] {
        let mut from = 0;
        while let Some(offset) = source[from..].find(marker) {
            let after = from + offset + marker.len();
            from = after;
            let rest = &bytes[after..];
            let mut index = 0;
            while rest.get(index).is_some_and(|b| b.is_ascii_whitespace()) {
                index += 1;
            }
            let Some(&quote) = rest.get(index) else {
                continue;
            };
            if quote != b'"' && quote != b'\'' {
                continue;
            }
            let tail = &source[after + index + 1..];
            let Some(end) = tail.find(quote as char) else {
                continue;
            };
            let name = &tail[..end];
            if !name.is_empty()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            {
                names.push(name.to_string());
            }
        }
    }
    names
}

fn unregistered_invocations(
    invoked: &BTreeMap<String, Vec<String>>,
    registered: &BTreeSet<String>,
) -> Vec<String> {
    invoked
        .iter()
        .filter(|(name, _)| !registered.contains(*name))
        .map(|(name, files)| format!("{name} (invoked by {})", files.join(", ")))
        .collect()
}

#[test]
fn every_ui_invoked_command_is_registered() {
    let registered = registered_commands();
    assert!(
        registered.contains("get_marks") && registered.contains("set_marks"),
        "generate_handler! parsing produced nonsense: {registered:?}"
    );

    let mut files = Vec::new();
    ui_files(&desktop_dir().join("ui"), &mut files);
    assert!(!files.is_empty(), "no UI files found under wol-desktop/ui");

    let mut invoked: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for file in &files {
        let source = fs::read_to_string(file).expect("read UI file");
        for name in invoked_commands(&source) {
            let label = file
                .strip_prefix(desktop_dir())
                .unwrap_or(file)
                .display()
                .to_string();
            invoked.entry(name).or_default().push(label);
        }
    }
    assert!(!invoked.is_empty(), "no invoke() calls found in wol-desktop/ui");

    let missing = unregistered_invocations(&invoked, &registered);

    assert!(
        missing.is_empty(),
        "UI invokes IPC commands that Rust does not register: {missing:?}\nregistered: {registered:?}"
    );
}

#[test]
fn detects_invoke_literals_and_stale_names() {
    let source = r#"
        await tauriInvoke("list_imported_publications");
        await invoke('get_languages');
        window.__TAURI_INTERNALS__.invoke("open_manager_window", {});
        await tauriInvoke(dynamicName);
        await invoke(`templated_${suffix}`);
    "#;
    let mut invoked: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for name in invoked_commands(source) {
        invoked.entry(name).or_default().push("ui/test.js".to_string());
    }
    assert_eq!(
        invoked.keys().cloned().collect::<Vec<_>>(),
        vec!["get_languages", "list_imported_publications", "open_manager_window"]
    );

    // The regression this file exists for: a `_command` suffix in Rust.
    let registered: BTreeSet<String> = [
        "get_languages".to_string(),
        "list_imported_publications_command".to_string(),
        "open_manager_window".to_string(),
    ]
    .into_iter()
    .collect();
    let missing = unregistered_invocations(&invoked, &registered);
    assert_eq!(missing.len(), 1, "{missing:?}");
    assert!(missing[0].starts_with("list_imported_publications ("), "{missing:?}");
}
