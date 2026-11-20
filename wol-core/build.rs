//! Build script: bakes the shipped Google OAuth client into the binary.
//!
//! Google's "Desktop app" OAuth clients require a `client_secret` on the token
//! exchange even when PKCE is used, so end users cannot sign in with a client_id
//! alone. Reading the value from `.env` at build time keeps it out of git while
//! still giving the built app a working "Sign in with Google Drive" button. The
//! only per-user secret is the user's own refresh token.
//!
//! Resolution order (first non-empty wins):
//!   1. `WOL_GOOGLE_CLIENT_ID` / `WOL_GOOGLE_CLIENT_SECRET` in the build environment
//!   2. `CHROME_CLIENT_ID` / `CHROME_CLIENT_SECRET` in the build environment
//!   3. the same keys in the workspace `.env` (override path with `WOL_ENV_FILE`)
//!
//! The values are exposed to the crate as `WOL_BUILTIN_GOOGLE_CLIENT_ID` and
//! `WOL_BUILTIN_GOOGLE_CLIENT_SECRET` (see `sync::resolve_google_credentials`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const ID_KEYS: [&str; 2] = ["WOL_GOOGLE_CLIENT_ID", "CHROME_CLIENT_ID"];
const SECRET_KEYS: [&str; 2] = ["WOL_GOOGLE_CLIENT_SECRET", "CHROME_CLIENT_SECRET"];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=WOL_ENV_FILE");
    for key in ID_KEYS.iter().chain(SECRET_KEYS.iter()) {
        println!("cargo:rerun-if-env-changed={key}");
    }

    let env_file = env_file_path();
    // Always watch the .env path even if it does not exist yet so creating it triggers a rebuild.
    println!("cargo:rerun-if-changed={}", env_file.display());
    let file_env = std::fs::read_to_string(&env_file)
        .map(|contents| parse_dotenv(&contents))
        .unwrap_or_default();

    println!(
        "cargo:rustc-env=WOL_BUILTIN_GOOGLE_CLIENT_ID={}",
        resolve(&ID_KEYS, &file_env)
    );
    println!(
        "cargo:rustc-env=WOL_BUILTIN_GOOGLE_CLIENT_SECRET={}",
        resolve(&SECRET_KEYS, &file_env)
    );
}

fn env_file_path() -> PathBuf {
    std::env::var_os("WOL_ENV_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new("../.env").to_path_buf())
}

fn resolve(keys: &[&str], file_env: &BTreeMap<String, String>) -> String {
    keys.iter()
        .map(|key| {
            std::env::var(key)
                .ok()
                .filter(|value| !value.trim().is_empty())
                .or_else(|| file_env.get(*key).cloned())
                .map(|value| value.trim().to_string())
                .unwrap_or_default()
        })
        .find(|value| !value.is_empty())
        .unwrap_or_default()
}

/// Minimal dotenv reader: `KEY=value`, optional `export` prefix, optional quotes.
fn parse_dotenv(contents: &str) -> BTreeMap<String, String> {
    contents
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let line = line.strip_prefix("export ").unwrap_or(line);
            let (key, value) = line.split_once('=')?;
            Some((key.trim().to_string(), unquote(value.trim()).to_string()))
        })
        .collect()
}

fn unquote(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() < 2 {
        return value;
    }
    let quoted = matches!((bytes[0], bytes[bytes.len() - 1]), (b'"', b'"') | (b'\'', b'\''));
    if quoted {
        &value[1..value.len() - 1]
    } else {
        value
    }
}
