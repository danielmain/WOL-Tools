//! Guards the `.manage()` ↔ `State<'_, T>` contract.
//!
//! Tauri resolves command state by *exact type*. `SyncManager` is managed as
//! `Arc<SyncManager>` (`lib.rs`: `.manage(sync_mgr.clone())`), so every command
//! that takes it must ask for `State<'_, Arc<SyncManager>>`. When those drift
//! apart nothing fails to compile — the IPCs just reject at runtime with
//! `state not managed for field ... on command ...`, and in the case of
//! `set_marks` that means highlights stop persisting without any visible error.

const LIB_RS: &str = include_str!("../src/lib.rs");
const COMMANDS_RS: &str = include_str!("../src/commands.rs");

#[test]
fn sync_manager_is_managed_as_arc_and_commands_ask_for_arc() {
    assert!(
        LIB_RS.contains(".manage(sync_mgr.clone())"),
        "lib.rs no longer manages sync_mgr (Arc<SyncManager>) — update command signatures too"
    );
    assert!(
        !COMMANDS_RS.contains("State<'_, SyncManager>"),
        "commands.rs asks for State<'_, SyncManager> but Arc<SyncManager> is managed; \
         use State<'_, Arc<SyncManager>> or the IPC is rejected at runtime"
    );
    assert!(
        COMMANDS_RS.contains("State<'_, Arc<SyncManager>>"),
        "set_marks/sync commands must take State<'_, Arc<SyncManager>>"
    );
}
