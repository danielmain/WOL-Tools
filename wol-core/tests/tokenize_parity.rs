//! Tokenization parity against `wol-highlights/lib/tokenize.js`.
//!
//! Token indices are the coordinates stored in every `wolMarks` backup, so the
//! Rust port in `wol_core::tokenize` must split text exactly like the shipped
//! content script. The fixtures are generated from the JS implementation by
//! `node wol-highlights/test/tokenize-parity.mjs --update`; this test replays
//! them so drift fails `cargo test` instead of silently mis-painting marks.

use std::path::PathBuf;

use serde_json::Value;
use wol_core::tokenize::{tokenize, utf16_len_of, Token};

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tokenize_cases.json")
}

fn load_fixture() -> Value {
    let raw = std::fs::read_to_string(fixture_path()).expect("tokenize fixture is readable");
    serde_json::from_str(&raw).expect("tokenize fixture is valid JSON")
}

fn expected_tokens(topic: &Value) -> Vec<Token> {
    topic["tokens"]
        .as_array()
        .expect("topic has a tokens array")
        .iter()
        .map(|token| {
            Token::new(
                usize::try_from(token["start"].as_u64().expect("start is a number")).expect("start fits"),
                usize::try_from(token["end"].as_u64().expect("end is a number")).expect("end fits"),
            )
        })
        .collect()
}

#[test]
fn rust_tokenizer_matches_the_extension_fixtures() {
    let fixture = load_fixture();
    let topics = fixture["topics"].as_array().expect("topics array");
    assert!(!topics.is_empty(), "fixture must contain cases");

    let mut failures = Vec::new();
    for topic in topics {
        let name = topic["name"].as_str().unwrap_or("<unnamed>");
        let text = topic["text"].as_str().expect("topic text");
        let expected = expected_tokens(topic);
        let actual = tokenize(text);
        if actual != expected {
            let first_diff = actual
                .iter()
                .zip(&expected)
                .position(|(left, right)| left != right)
                .unwrap_or_else(|| actual.len().min(expected.len()));
            failures.push(format!(
                "{name}: {len} tokens vs {expected_len} expected, first difference at #{first_diff} (rust {rust:?}, js {js:?})",
                len = actual.len(),
                expected_len = expected.len(),
                rust = actual.get(first_diff),
                js = expected.get(first_diff),
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "tokenizer diverged from tokenize.js:\n{}",
        failures.join("\n")
    );
}

#[test]
fn fixture_tokens_stay_inside_the_utf16_text() {
    let fixture = load_fixture();
    for topic in fixture["topics"].as_array().expect("topics array") {
        let name = topic["name"].as_str().unwrap_or("<unnamed>");
        let text = topic["text"].as_str().expect("topic text");
        let length = utf16_len_of(text);
        let tokens = expected_tokens(topic);
        for (index, token) in tokens.iter().enumerate() {
            assert!(
                token.start < token.end && token.end <= length,
                "{name}: token #{index} {token:?} is out of bounds (text is {length} UTF-16 units)"
            );
            if let Some(previous) = index.checked_sub(1).and_then(|at| tokens.get(at)) {
                assert!(
                    previous.end <= token.start,
                    "{name}: token #{index} overlaps its predecessor"
                );
            }
        }
    }
}
