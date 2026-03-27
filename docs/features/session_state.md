# Session State Persistence

## Scope

**In scope:**
- Persist filter mode, status filter, list mode, sort mode, focused pane, pane tree layout, and selected service
- Restore state on startup with fallback to config defaults
- Save state on every state-modifying action (filter cycling, pane operations, service selection, etc.)
- Configurable Ctrl-r hotkey to reset state to defaults with confirmation dialog
- TOML-based serialization at `$XDG_STATE_HOME/sysdui/session.toml`

**Not in scope:**
- Persisting log buffer contents (logs are re-streamed on restore)
- Persisting scroll offsets or search queries
- Persisting unit detail cache
- Multi-session / named session support

## Data Flow

### Save Path
1. User performs a state-modifying action (e.g., cycles filter, splits pane, selects service)
2. The action handler in `App` calls `self.save_state()`
3. `save_state()` constructs a `SessionState` from current `App` fields
4. `SerializedPaneNode::from_pane_node()` walks the `PaneTree` recursively, converting each node
5. `save_session()` serializes to TOML and writes to `state_path()`

### Load Path
1. `App::new()` calls `crate::state::load_session()`
2. If a session file exists, it is deserialized into `SessionState`
3. Filter/sort/list modes are parsed from strings back to enums
4. `session.to_pane_tree()` reconstructs the `PaneTree` (with empty log buffers)
5. After `load_units()` and `apply_filters()`, the selected service index is restored
6. `start_all_journal_streams()` iterates all pane leaves and spawns journal streams

### Reset Path
1. User presses Ctrl-r (or configured key for `ResetState`)
2. A `ConfirmDialog::new_reset()` is shown
3. On confirmation, `App::reset_state()` is called
4. All journal handles are aborted
5. All state fields are reset to config defaults
6. Session file is deleted from disk
7. Units are reloaded and filters reapplied

## Files

| File | Role | Key exports/interfaces |
|------|------|----------------------|
| `src/state.rs` | Serializable state types and file I/O | `SessionState`, `SerializedPaneNode`, `save_session()`, `load_session()`, `delete_session()` |
| `src/app.rs` | State persistence integration | `App::save_state()`, `App::start_all_journal_streams()`, `App::reset_state()`, modified `App::new()` |
| `src/config/keys.rs` | Reset state key binding | `KeyAction::ResetState`, default Ctrl-r binding |
| `src/ui/confirm.rs` | Reset confirmation dialog | `ConfirmAction::ResetState`, `ConfirmDialog::new_reset()` |
| `src/main.rs` | Module declaration | `mod state;` |

## Invariants and Constraints

- `SerializedPaneNode` uses `#[serde(tag = "type")]` with named fields (`left`/`right`) instead of tuple variants for TOML compatibility
- `PaneLeaf` fields that are runtime-only (log_buffer, scroll_offset, search_query, journal_handle) are not serialized; they are initialized to defaults on restore
- Priority values round-trip through `as_journalctl_arg()` / `Priority::from_str()`
- The `save_state()` method logs warnings on failure but never panics or propagates errors
- Session restore is best-effort: if the file is corrupt or missing, config defaults are used
- The `state_path()` function falls back to `~/.local/state` if `dirs::state_dir()` returns None
- `save_state()` is called after every state-modifying action in both keyboard and mouse handlers
