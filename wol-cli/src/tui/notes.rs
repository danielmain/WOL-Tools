//! Inline note viewer & editor.
//!
//! Notes live inside `wolMarks` exactly like the extension stores them, keyed by
//! document id + `data-pid`, so an edit made here is picked up by the desktop
//! app and the browser on their next read.

use wol_core::backup::{NoteItem, StoredIndex};
use wol_core::state::now_iso_utc;

/// Read the notes for one paragraph.
pub fn notes_for(marks: &StoredIndex, document_id: &str, pid: &str) -> Vec<NoteItem> {
    marks.doc_notes(document_id, pid).to_vec()
}

/// Replace the note list for a paragraph with `content` (one note, or remove).
pub fn upsert_note(
    marks: &mut StoredIndex,
    document_id: &str,
    pid: &str,
    content: Option<&str>,
) {
    let paragraphs = marks.notes.docs.entry(document_id.to_string()).or_default();
    match content {
        Some(text) if !text.trim().is_empty() => {
            let now = now_iso_utc();
            let list = paragraphs.entry(pid.to_string()).or_default();
            match list.first_mut() {
                Some(existing) => {
                    existing.content = text.trim().to_string();
                    existing.last_modified = Some(now);
                }
                None => list.push(new_note(pid, text.trim(), &now)),
            }
        }
        _ => {
            paragraphs.remove(pid);
            if paragraphs.is_empty() {
                marks.notes.docs.remove(document_id);
            }
        }
    }
    *marks = wol_core::backup::recount(marks);
}

fn new_note(pid: &str, content: &str, now: &str) -> NoteItem {
    NoteItem {
        note_id: None,
        guid: Some(uuid::Uuid::new_v4().to_string()),
        user_mark_id: None,
        location_id: None,
        title: None,
        content: content.to_string(),
        last_modified: Some(now.to_string()),
        created: Some(now.to_string()),
        block_type: Some(1),
        block_identifier: pid.parse().ok(),
    }
}

/// Result of handling a key inside the note editor.
#[derive(Debug, PartialEq, Eq)]
pub enum EditorOutcome {
    /// Still editing.
    Continue,
    /// Save with this text.
    Save(String),
    /// Close without saving.
    Cancel,
}

/// Single-line-ish editable buffer for the note popup.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoteEditor {
    pub buffer: String,
    pub cursor: usize,
    pub paragraph: Option<String>,
}

impl NoteEditor {
    pub fn open(paragraph: &str, existing: &str) -> Self {
        let mut editor = Self {
            buffer: existing.to_string(),
            cursor: 0,
            paragraph: Some(paragraph.to_string()),
        };
        editor.cursor = editor.buffer.chars().count();
        editor
    }

    pub fn insert(&mut self, character: char) {
        let index = self.cursor.min(self.buffer.chars().count());
        let mut chars: Vec<char> = self.buffer.chars().collect();
        chars.insert(index, character);
        self.buffer = chars.into_iter().collect();
        self.cursor = index + 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let index = (self.cursor - 1).min(self.buffer.chars().count());
        let mut chars: Vec<char> = self.buffer.chars().collect();
        chars.remove(index);
        self.buffer = chars.into_iter().collect();
        self.cursor = index;
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.buffer.chars().count());
    }

    /// Ctrl-U / Esc are cancel; Enter saves; empty buffer saves as "remove".
    pub fn apply(&mut self, key: EditorKey) -> EditorOutcome {
        match key {
            EditorKey::Enter => EditorOutcome::Save(self.buffer.clone()),
            EditorKey::Cancel => EditorOutcome::Cancel,
            EditorKey::Char(character) => {
                self.insert(character);
                EditorOutcome::Continue
            }
            EditorKey::Backspace => {
                self.backspace();
                EditorOutcome::Continue
            }
            EditorKey::Left => {
                self.left();
                EditorOutcome::Continue
            }
            EditorKey::Right => {
                self.right();
                EditorOutcome::Continue
            }
        }
    }

    /// Buffer split around the cursor for rendering.
    pub fn parts(&self) -> (String, String) {
        let chars: Vec<char> = self.buffer.chars().collect();
        let index = self.cursor.min(chars.len());
        (
            chars[..index].iter().collect(),
            chars[index..].iter().collect(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorKey {
    Char(char),
    Backspace,
    Left,
    Right,
    Enter,
    Cancel,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_and_updates_notes() {
        let mut marks = StoredIndex::default();
        upsert_note(&mut marks, "2025441", "7", Some("first"));
        assert_eq!(marks.doc_notes("2025441", "7").len(), 1);
        assert_eq!(marks.note_count, 1);

        upsert_note(&mut marks, "2025441", "7", Some("second"));
        let notes = marks.doc_notes("2025441", "7");
        assert_eq!(notes.len(), 1, "editing must not duplicate the note");
        assert_eq!(notes[0].content, "second");

        upsert_note(&mut marks, "2025441", "7", None);
        assert!(marks.doc_notes("2025441", "7").is_empty());
        assert!(marks.notes.docs.is_empty());
        assert_eq!(marks.note_count, 0);
    }

    #[test]
    fn empty_content_removes_the_note() {
        let mut marks = StoredIndex::default();
        upsert_note(&mut marks, "doc", "1", Some("text"));
        upsert_note(&mut marks, "doc", "1", Some("   "));
        assert!(marks.notes.docs.is_empty());
    }

    #[test]
    fn editor_inserts_and_deletes_at_the_cursor() {
        let mut editor = NoteEditor::open("7", "abc");
        assert_eq!(editor.parts(), ("abc".to_string(), String::new()));
        editor.left();
        editor.insert('X');
        assert_eq!(editor.buffer, "abXc");
        editor.backspace();
        assert_eq!(editor.buffer, "abc");
        assert_eq!(editor.apply(EditorKey::Enter), EditorOutcome::Save("abc".into()));
        assert_eq!(editor.apply(EditorKey::Cancel), EditorOutcome::Cancel);
    }
}
