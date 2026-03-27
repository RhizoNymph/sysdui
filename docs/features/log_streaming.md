# Log Streaming

## Scope

### In Scope

- Spawning `journalctl -f` processes to tail live logs for systemd units
- Buffering log output per-pane in a bounded ring buffer
- Scrolling through log history (keyboard and mouse)
- Auto-follow (tail) mode with a LIVE indicator
- Scroll-back mode with a "+N new" indicator showing lines below the viewport
- Case-insensitive text search within log lines with match highlighting
- Priority (log level) filtering that cycles through err/warning/notice/info/debug
- Stream lifecycle management (spawn, kill on pane close, restart on filter change)
- Per-pane independent log state (buffer, scroll position, search query, priority filter)

### Not In Scope

- Pane layout, splitting, or tree management (see pane system)
- Service browsing, filtering, or sidebar navigation
- D-Bus communication or unit property monitoring
- Service control actions (start/stop/restart/enable/disable)

## Data/Control Flow

### 1. Spawning a Journal Stream

When a service is selected or a pane is created, `App::start_journal_for_pane()` is called. This method:

1. Checks that `service_name` is non-empty (returns early if empty).
2. Kills any existing journal stream for the target pane by aborting the stored `JoinHandle`.
3. Calls `journal::spawn_journal_stream()` with the service name, bus type, priority, pane ID, and event channel sender.
4. Stores the returned `JoinHandle` in `pane.journal_handle`.

`spawn_journal_stream()` builds and spawns a `journalctl` command:

```
journalctl -f -u <unit_name> -o short-iso --no-pager --priority=<level> [--user]
```

- `-f` enables follow mode (tail).
- `-o short-iso` sets the output format.
- `--priority=<level>` filters to the specified priority and above.
- `--user` is added when `bus_type == BusType::Session`.
- stdout is piped; stderr is discarded.

A tokio task reads lines from stdout in a loop and sends each as `AppEvent::LogLine { pane_id, line }` over the unbounded channel. When the stream ends (EOF or read error), it sends `AppEvent::LogStreamEnded { pane_id }` and kills the child process.

### 2. Log Line Ingestion

The app event loop receives `AppEvent::LogLine { pane_id, line }` and calls `handle_log_line()`, which locates the pane by ID and calls `pane.push_line(line)`.

`PaneLeaf::push_line()` manages the ring buffer:

1. If `log_buffer.len() >= MAX_LOG_LINES` (10,000), evict the oldest line via `pop_front()`.
2. When evicting while the user is scrolled back (`scroll_offset > 0`), decrement `scroll_offset` by 1 so the viewport continues to point at the same content despite the front of the buffer shifting.
3. Append the new line to the back of the buffer.

### 3. Scroll Offset and Viewport Calculation

`scroll_offset` represents how many lines back from the tail the viewport is positioned:

- **`scroll_offset == 0`**: Auto-follow (tail) mode. The viewport shows the last `visible_height` lines.
- **`scroll_offset > 0`**: Scrolled back. The viewport start is calculated as `total_lines - visible_height - scroll_offset`.

Because the start position is computed relative to `total_lines`, the viewport shifts forward as new lines arrive even when scrolled back. The scroll_offset maintains a fixed distance from the tail, so the "+N new" indicator accurately reflects how many unseen lines exist below the viewport.

Scroll controls:

| Input             | Action                         | scroll_offset change |
|-------------------|--------------------------------|----------------------|
| Arrow Up / `k`    | Scroll up 1 line               | +1                   |
| Arrow Down / `j`  | Scroll down 1 line             | -1 (saturating)      |
| Page Up           | Scroll up 20 lines             | +20                  |
| Page Down         | Scroll down 20 lines           | -20 (saturating)     |
| Mouse scroll up   | Scroll up 3 lines (in pane)    | +3                   |
| Mouse scroll down | Scroll down 3 lines (in pane)  | -3 (saturating)      |

Scrolling down past `scroll_offset == 0` is a no-op (saturating subtraction), returning the user to tail mode.

### 4. LIVE and "+N new" Indicators

The log pane title bar shows:

- **`[LIVE]`** when `pane.is_following()` returns true (i.e., `scroll_offset == 0`). The user is seeing the latest output in real time.
- **`[+N new]`** when `scroll_offset > 0`, where N is the scroll_offset value. This tells the user how many lines exist below their current viewport.

The title also includes the service name and current priority filter: `" Logs: <service> [<priority>] "`.

### 5. Log Search

Activated by `Ctrl-/` (enters `InputMode::SearchLogs`).

**While in SearchLogs mode:**
- Character keys append to `app.search_query`.
- Backspace removes the last character.
- Enter commits: copies `app.search_query` to `pane.search_query` on the focused pane, returns to Normal mode.
- Esc cancels: clears both `app.search_query` and `pane.search_query`, returns to Normal mode.

**Rendering with search active (`pane.search_query` non-empty):**

Each visible log line is processed by `filter::find_matches()`, which performs case-insensitive substring matching and returns byte-range pairs of all occurrences. Matched spans are rendered with a yellow background and black foreground. Non-matching portions render as plain text.

### 6. Priority Filter Cycling

Activated by the `l` key (`KeyAction::CycleLogLevel`).

The priority cycles through: `err -> warning -> notice -> info -> debug -> err`.

When cycled:
1. `pane.priority_filter` is updated to the next value via `Priority::cycle_next()`.
2. The existing journal handle is aborted.
3. The log buffer is cleared.
4. A new journal stream is spawned with the updated priority filter.

This is a full stream restart because `journalctl --priority=` is a command-line argument that cannot be changed on a running process.

### 7. Stream End Handling

When `AppEvent::LogStreamEnded { pane_id }` is received, `handle_log_stream_ended()` sets `pane.journal_handle = None`. The log buffer is preserved so the user can still scroll through previously received lines.

### 8. Service Change on Focused Pane

When the user selects a different service in the sidebar (`select_service()`):
1. The focused pane's `service_name` is updated.
2. The log buffer is cleared.
3. `scroll_offset` is reset to 0 (tail mode).
4. The old journal handle is aborted.
5. A new journal stream is started for the new service.

### 9. Pane Close Cleanup

When a pane is closed via `KeyAction::ClosePane`, the `close_node()` function in the pane tree aborts the pane's `journal_handle` before removing the node. This ensures the `journalctl` child process is killed.

## Related Files

| File | Role | Key Exports/Interfaces |
|---|---|---|
| `src/journal/mod.rs` | Spawns `journalctl` child processes, reads stdout, sends events | `spawn_journal_stream()` |
| `src/journal/filter.rs` | Priority enum and log search matching | `Priority` (enum with `cycle_next()`, `as_journalctl_arg()`, `from_str()`), `find_matches()` |
| `src/ui/logs.rs` | Renders the log pane widget with scrollbar and search highlighting | `render_log_pane()` |
| `src/ui/panes.rs` | Pane data structures, log buffer management, pane tree operations | `PaneLeaf` (struct), `PaneId` (type alias), `PaneTree` (struct), `MAX_LOG_LINES` (const), `PaneLeaf::push_line()`, `PaneLeaf::is_following()` |
| `src/app.rs` | Orchestrates log streaming lifecycle, handles events, manages input modes | `App::start_journal_for_pane()`, `App::handle_log_line()`, `App::handle_log_stream_ended()`, `App::select_service()`, `InputMode::SearchLogs` |
| `src/event.rs` | Defines the event types for log data flow | `AppEvent::LogLine { pane_id, line }`, `AppEvent::LogStreamEnded { pane_id }` |
| `src/config/keys.rs` | Keybinding definitions for log-related actions | `KeyAction::SearchLogs`, `KeyAction::CycleLogLevel`, `KeyAction::ScrollUp/Down`, `KeyAction::PageUp/Down` |
| `src/ui/help.rs` | Help overlay entries for log-related keys | References `KeyAction::SearchLogs`, `KeyAction::CycleLogLevel` |
| `src/ui/mod.rs` | Top-level UI rendering, search input bar for SearchLogs mode | `InputMode::SearchLogs` rendering |

## Invariants and Constraints

1. **Log buffer capped at 10,000 lines per pane.** Enforced by `MAX_LOG_LINES` in `PaneLeaf::push_line()`. Oldest lines are evicted via `pop_front()` when the cap is reached. The buffer is pre-allocated with `VecDeque::with_capacity(MAX_LOG_LINES)`.

2. **`scroll_offset == 0` means auto-follow (tail mode).** This is the semantic contract used by `is_following()`, the viewport calculation in `render_log_pane()`, and the LIVE/+N indicator logic. All scroll modifications use saturating arithmetic to prevent underflow past zero.

3. **`scroll_offset` is adjusted on eviction.** When lines are evicted from the front of the buffer while `scroll_offset > 0`, the offset is decremented by 1 to keep the viewport pointing at the same logical content.

4. **Journal stream is killed when pane is closed.** The `close_node()` function in the pane tree calls `abort()` on the `journal_handle` before removing the pane. This ensures the `journalctl` child process is terminated.

5. **Priority filter change requires stream restart.** Because the priority is a `journalctl` command-line argument, changing it requires killing the old process, clearing the buffer, and spawning a new one. There is no way to change the filter on a running stream.

6. **Each pane has its own independent log state.** `PaneLeaf` owns its own `log_buffer`, `scroll_offset`, `search_query`, `priority_filter`, and `journal_handle`. No state is shared between panes.

7. **Service change clears state.** When the focused pane switches to a different service, the log buffer is cleared, scroll_offset is reset to 0, and the old stream is killed before starting a new one.

8. **Stream end preserves buffer.** When `LogStreamEnded` is received, the handle is set to `None` but the buffer contents are kept intact for the user to review.

9. **One journal stream per pane.** `start_journal_for_pane()` always aborts any existing handle before spawning a new stream, preventing duplicate streams on the same pane.

10. **Event channel is unbounded.** `spawn_journal_stream()` uses `mpsc::UnboundedSender<AppEvent>`, so log lines are never dropped due to backpressure. If the sender is dropped (app shutting down), the stream loop breaks.
