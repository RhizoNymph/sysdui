# Event System

## Scope

### In Scope

- The `AppEvent` enum defining all event variants that flow through the application
- The `EventHandler` struct that owns an unbounded mpsc channel and provides the `sender()` / `next()` interface
- Three built-in event producer tasks:
  - `spawn_terminal_reader()` -- crossterm terminal input at native rate
  - `spawn_tick_timer()` -- periodic reconciliation tick at ~4 Hz (250 ms)
  - `spawn_render_timer()` -- periodic screen refresh at ~30 Hz (33 ms)
- External event producers that share the same channel:
  - D-Bus signal listeners (`dbus::spawn_signal_listener`) forwarding `UnitNew`, `UnitRemoved`, and `PropertiesChanged`
  - Journal stream processes (`journal::spawn_journal_stream`) forwarding `LogLine` and `LogStreamEnded`
- The multiplexing pattern: many producers, one consumer, single sequential main loop
- The top-level dispatch in `App::handle_event()` that routes each `AppEvent` variant to the appropriate handler

### Not In Scope

- Specific event handling logic within individual handlers (e.g., what `handle_key` does with a particular keybinding)
- UI rendering (the `Render` event is consumed in `main.rs`, not in `handle_event`)
- D-Bus connection setup, signal subscription rules, or unit data model
- Journal process management and log filtering
- TUI initialization and terminal management (covered by `tui.rs`)

---

## Data / Control Flow

### 1. Channel Creation

`EventHandler::new()` creates a `tokio::sync::mpsc::unbounded_channel()` and stores both the sender (`tx`) and receiver (`rx`). The `sender()` method clones and returns the `tx` side so that multiple producers can share it.

```
EventHandler::new()
  -> mpsc::unbounded_channel() -> (tx, rx)
  -> stores both in EventHandler { tx, rx }

EventHandler::sender()
  -> self.tx.clone()
```

### 2. Built-in Event Producers

All three are spawned as independent `tokio::spawn` tasks during startup in `main.rs`:

```
events.spawn_terminal_reader();
events.spawn_tick_timer();
events.spawn_render_timer();
```

**Terminal Reader** (`spawn_terminal_reader`):
- Creates a `crossterm::event::EventStream` (an async adapter over crossterm's blocking event poll)
- Loops with `reader.next().await`, wrapping each `crossterm::event::Event` in `AppEvent::Terminal(event)` and sending it on the channel
- Breaks if the send fails (channel closed, meaning the receiver was dropped)

**Tick Timer** (`spawn_tick_timer`):
- Creates a `tokio::time::interval(250ms)` (~4 Hz)
- On each tick, sends `AppEvent::Tick`
- Used for lazy reconciliation: fetching details for the currently selected unit if not already cached

**Render Timer** (`spawn_render_timer`):
- Creates a `tokio::time::interval(33ms)` (~30 Hz)
- On each tick, sends `AppEvent::Render`
- The main loop uses this event to trigger `terminal.draw()`

### 3. External Event Producers

**D-Bus Signal Listeners** (`dbus::spawn_signal_listener`):
- Spawned once per bus (system and session) in `main.rs` with a cloned sender
- Internally spawns two sub-tasks per bus:
  1. A `PropertiesChanged` signal stream on `org.freedesktop.DBus.Properties` -- sends `AppEvent::PropertiesChanged { path, bus_type, changed }`
  2. A manager signal stream on `org.freedesktop.systemd1.Manager` -- sends `AppEvent::UnitNew { name, path, bus_type }` or `AppEvent::UnitRemoved { name, path, bus_type }` based on the signal member name

**Journal Streams** (`journal::spawn_journal_stream`):
- Spawned on-demand when a pane starts tailing a unit's journal
- Runs `journalctl -f -u <unit>` as a child process
- Reads stdout line-by-line, sending `AppEvent::LogLine { pane_id, line }` for each line
- Sends `AppEvent::LogStreamEnded { pane_id }` when the stream ends (EOF or error)
- Kills the child process on termination

**CommandResult** (`AppEvent::CommandResult`):
- Defined in the enum but not currently sent by any producer in the codebase
- The handler in `App::handle_event()` logs it via `tracing::info!`

### 4. Event Consumption

`EventHandler::next()` calls `self.rx.recv().await`, returning `Option<AppEvent>`. Returns `None` when all senders are dropped (channel closed).

### 5. Main Event Loop (`main.rs`)

```
loop {
    let Some(event) = events.next().await else { break };
    let is_render = matches!(event, AppEvent::Render);

    app.handle_event(event).await;

    if app.should_quit { break; }

    // Handle TUI suspend for blocking operations (sudo, editor)
    if let Some(action) = app.needs_tui_suspend.take() {
        tui::suspend()?;
        App::execute_suspended_action(&action);
        terminal = tui::resume()?;
        app.load_units().await;
        app.apply_filters();
    }

    // Only draw on render events
    if is_render {
        terminal.draw(|frame| { ... });
    }
}
```

Key behaviors:
- Events are consumed **one at a time**, sequentially
- `handle_event()` is called for every event, including `Render` (which is a no-op inside the handler)
- Rendering only happens when the event was `AppEvent::Render`
- TUI suspend/resume is handled inline between events, which implicitly pauses event processing (the terminal reader continues running in its own task but the crossterm `EventStream` reads from a raw-mode terminal which is disabled during suspend)

### 6. Top-Level Dispatch (`App::handle_event`)

```
match event {
    AppEvent::Terminal(Event::Key(key))     => self.handle_key(key).await,
    AppEvent::Terminal(Event::Mouse(mouse)) => self.handle_mouse(mouse).await,
    AppEvent::Terminal(Event::Resize(_, _)) => {},  // re-render handles this
    AppEvent::Tick                          => self.handle_tick().await,
    AppEvent::Render                        => {},  // handled in main loop
    AppEvent::UnitNew { name, bus_type, .. }         => self.handle_unit_new(&name, bus_type).await,
    AppEvent::UnitRemoved { name, .. }               => self.handle_unit_removed(&name),
    AppEvent::PropertiesChanged { path, bus_type, changed } => self.handle_properties_changed(&path, bus_type, &changed).await,
    AppEvent::LogLine { pane_id, line }     => self.handle_log_line(pane_id, line),
    AppEvent::LogStreamEnded { pane_id }    => self.handle_log_stream_ended(pane_id),
    AppEvent::CommandResult { action, result } => tracing::info!("Command {action}: {result:?}"),
    _ => {}
}
```

---

## AppEvent Variants

| Variant | Producer | Purpose |
|---------|----------|---------|
| `Terminal(Event)` | `spawn_terminal_reader` | Wraps crossterm key, mouse, and resize events |
| `Tick` | `spawn_tick_timer` | Triggers lazy data reconciliation (~4 Hz) |
| `Render` | `spawn_render_timer` | Triggers screen redraw (~30 Hz) |
| `UnitNew { name, path, bus_type }` | `dbus::spawn_signal_listener` | A systemd unit appeared on the bus |
| `UnitRemoved { name, path, bus_type }` | `dbus::spawn_signal_listener` | A systemd unit was removed from the bus |
| `PropertiesChanged { path, bus_type, changed }` | `dbus::spawn_signal_listener` | A unit's D-Bus properties changed |
| `LogLine { pane_id, line }` | `journal::spawn_journal_stream` | A new journal log line for a pane |
| `LogStreamEnded { pane_id }` | `journal::spawn_journal_stream` | Journal stream for a pane terminated |
| `CommandResult { action, result }` | (unused) | Result of an async command execution |

---

## Related Files

| File | Role | Key Exports / Interfaces |
|------|------|--------------------------|
| `src/event.rs` | Core event types and handler | `AppEvent` (enum), `EventHandler` (struct with `new()`, `sender()`, `next()`, `spawn_terminal_reader()`, `spawn_tick_timer()`, `spawn_render_timer()`) |
| `src/main.rs` | Main event loop, startup wiring | Calls `EventHandler::new()`, spawns all producers, runs `loop { events.next() -> app.handle_event() }`, handles render and TUI suspend |
| `src/app.rs` | Top-level event dispatch and all handler methods | `App::handle_event()`, `handle_key()`, `handle_mouse()`, `handle_tick()`, `handle_unit_new()`, `handle_unit_removed()`, `handle_properties_changed()`, `handle_log_line()`, `handle_log_stream_ended()` |
| `src/systemd/dbus.rs` | D-Bus signal listener producer | `spawn_signal_listener(conn, bus_type, tx)` -- spawns tasks that send `UnitNew`, `UnitRemoved`, `PropertiesChanged` events |
| `src/journal/mod.rs` | Journal stream producer | `spawn_journal_stream(unit_name, bus_type, priority, pane_id, tx)` -- spawns a `journalctl -f` child process that sends `LogLine` and `LogStreamEnded` events |
| `src/tui.rs` | Terminal init/suspend/resume | `init()`, `restore()`, `suspend()`, `resume()` -- manages raw mode and alternate screen; suspend/resume affects terminal reader behavior |
| `src/ui/panes.rs` | Pane identity type | `PaneId` -- used by `LogLine` and `LogStreamEnded` to target the correct pane |
| `src/systemd/types.rs` | Bus type enum | `BusType` (System / Session) -- used by `UnitNew`, `UnitRemoved`, `PropertiesChanged` |

---

## Invariants and Constraints

1. **Single channel, many producers**: All event producers (terminal reader, tick timer, render timer, D-Bus listeners, journal streams) share clones of the same `mpsc::UnboundedSender<AppEvent>`. There is exactly one receiver, owned by `EventHandler`.

2. **Sequential event processing**: The main loop processes events one at a time. `handle_event()` is `async` but is `.await`-ed inline, meaning no two events are ever processed concurrently. This eliminates the need for interior mutability or locking on `App` state.

3. **Render timer must be faster than tick timer**: The render timer fires at ~30 Hz (33 ms) and the tick timer at ~4 Hz (250 ms). This ensures the UI stays visually responsive while reconciliation work happens at a lower cadence. Violating this would cause visible lag between state changes and their display.

4. **Unbounded channel prevents backpressure**: The channel is `mpsc::unbounded_channel`, meaning producers never block when sending. This is critical because:
   - The terminal reader runs on crossterm's event stream and must not be stalled
   - D-Bus signal streams are externally driven and could drop signals if blocked
   - Journal stream child processes could deadlock if their stdout pipe fills up due to backpressure

5. **Terminal reader behavior during TUI suspend**: When `tui::suspend()` is called, raw mode is disabled and the alternate screen is left. The terminal reader task continues running in its tokio runtime, but `crossterm::event::EventStream` will not produce meaningful TUI events while raw mode is off. After `tui::resume()` re-enables raw mode, the reader resumes normal operation. The main loop blocks synchronously during the suspend (reading stdin for "Press Enter") so no events are dispatched during that window.

6. **Channel closure signals shutdown**: When all senders are dropped (or equivalently, when `events.next()` returns `None`), the main loop exits. In practice, shutdown is triggered by `app.should_quit = true` causing a `break` before channel closure.

7. **Producer self-termination on send failure**: Every producer checks `tx.send(...).is_err()` and breaks out of its loop if the send fails. This ensures that once the receiver is dropped (main loop exited), all spawned tasks clean up promptly rather than spinning forever.
