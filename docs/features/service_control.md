# Service Control

## Scope

### In Scope

- **Service lifecycle actions**: Start, Stop, Restart, Enable, Disable, and Daemon-Reload via `systemctl`.
- **Unit file editing**: Opening a unit's fragment file in `$EDITOR` (falls back to `vi`).
- **Privilege escalation**: Automatically prefixing `sudo` for system-bus services (both `systemctl` commands and editor invocations).
- **Confirmation dialogs**: A configurable confirmation overlay that gates destructive or stateful actions before execution.
- **TUI suspend/resume**: Cleanly leaving the alternate screen so that external processes (`sudo`, `$EDITOR`, `systemctl`) can interact with the terminal, then restoring the TUI afterwards.
- **Post-action reload**: Re-fetching the unit list from D-Bus after any service action completes so the UI reflects the new state.
- **Context menu integration**: Right-click context menus on sidebar services expose the same Start/Stop/Restart/Enable/Disable actions.

### Out of Scope

- Browsing/listing units (handled by the filtering and sidebar subsystems).
- Journal/log viewing (handled by the journal streaming subsystem).
- Pane splitting and layout management (separate feature, though context menus share UI surface).

---

## Data/Control Flow

### 1. User triggers an action

Actions are triggered in two ways:

1. **Keyboard shortcut** in `InputMode::Normal`: The keybinding system maps keys (default: `s`=Start, `x`=Stop, `r`=Restart, `n`=Enable, `d`=Disable, `o`=DaemonReload, `e`=EditUnit) to `KeyAction` variants. When matched, `App::handle_key_normal()` calls either `request_action(ServiceAction)` or `edit_unit()`.

2. **Context menu**: Right-clicking a sidebar service opens a `ContextMenu` whose items include `ContextMenuAction::ServiceAction(ServiceAction::*)`. When selected, `App::handle_context_menu_action()` calls `request_action_for_unit(sa, unit_name)`.

### 2. Confirmation check

Both `request_action()` and `request_action_for_unit()` follow the same pattern:

```
fn request_action(&mut self, action: ServiceAction) {
    // 1. Resolve the target unit name (from selected unit; empty for DaemonReload)
    // 2. Check config
    if self.config.needs_confirmation(action.confirm_key()) {
        // Show confirmation dialog
        self.confirm_dialog = Some(ConfirmDialog::new(action, unit_name));
        self.input_mode = InputMode::Confirm;
    } else {
        // Skip dialog, execute immediately
        self.execute_action_with_name(action, unit_name_option);
    }
}
```

`Config::needs_confirmation()` first checks `confirmations.global` -- if `false`, all confirmations are skipped. Otherwise it checks the per-action boolean (`confirmations.start`, `confirmations.stop`, etc.). All default to `true`.

### 3. Confirmation dialog display and resolution

When `input_mode == InputMode::Confirm`, the render loop draws the `ConfirmDialog` overlay centered on screen. The dialog shows the action label, the unit name (if applicable), and `[y]es / any key to cancel`.

Key handling in `InputMode::Confirm`:
- `KeyAction::Confirm` (default: `y`): Takes the dialog via `confirm_dialog.take()`, calls `execute_action(dialog)`, returns to `InputMode::Normal`.
- **Any other key**: Sets `confirm_dialog = None`, returns to `InputMode::Normal` (action is cancelled).

### 4. Setting `needs_tui_suspend`

`execute_action()` extracts the action and unit name from the `ConfirmDialog`, then delegates to `execute_action_with_name()`.

`execute_action_with_name()` resolves the `BusType` (System or Session) by looking up the unit in `all_units`, then sets:

```rust
self.needs_tui_suspend = Some(SuspendAction::Systemctl {
    action,
    unit_name,
    bus_type,
});
```

For `edit_unit()`, the flow is:
1. Look up the selected unit's `ServiceDetail` to get `fragment_path`.
2. If `fragment_path` is empty, return early (no file to edit).
3. Set `self.needs_tui_suspend = Some(SuspendAction::EditUnit { fragment_path, bus_type })`.

### 5. Main loop: suspend, execute, resume

In `main.rs`, after each event is handled:

```rust
if let Some(action) = app.needs_tui_suspend.take() {
    tui::suspend()?;                              // Step A
    let result = App::execute_suspended_action(&action);  // Step B
    if let Err(e) = &result {
        eprintln!("Error: {e}");
        eprintln!("Press Enter to continue...");
        let _ = std::io::stdin().read_line(&mut buf);     // Step C (error display)
    }
    terminal = tui::resume()?;                    // Step D
    let _ = app.load_units().await;               // Step E
    app.apply_filters();                          // Step F
}
```

- **Step A -- `tui::suspend()`**: Leaves the alternate screen (`LeaveAlternateScreen`), disables mouse capture, shows the cursor, and disables raw mode. The terminal is now in normal cooked mode.
- **Step B -- `execute_suspended_action()`**: Dispatches to either `execute_systemctl()` or `edit_unit_file()` (see below). These are blocking `std::process::Command` calls.
- **Step C**: On error, the message is printed to stderr and the user must press Enter before the TUI is restored.
- **Step D -- `tui::resume()`**: Re-initializes the TUI (enters alternate screen, enables mouse capture, hides cursor, enables raw mode). Returns a fresh `Terminal` handle which replaces the old one.
- **Step E/F -- Reload**: `load_units()` re-fetches all units from both D-Bus connections. `apply_filters()` re-applies the current filter/sort state so the sidebar reflects any changes.

### 6. Command execution details

#### `execute_systemctl(action, unit_name, bus_type)`

Builds a `std::process::Command`:

| Bus Type | Command built |
|----------|--------------|
| `BusType::System` | `sudo systemctl <verb> [unit_name]` |
| `BusType::Session` | `systemctl --user <verb> [unit_name]` |

- The verb comes from `ServiceAction::verb()` (e.g., `"start"`, `"daemon-reload"`).
- `unit_name` is only appended if `action.needs_unit()` returns `true` (all actions except `DaemonReload`).
- Uses `.status()` (not `.output()`), so stdin/stdout/stderr are inherited -- necessary for `sudo` password prompts.
- Returns `Ok(label + " succeeded")` or bails with the exit code on failure.

#### `edit_unit_file(fragment_path, bus_type)`

- Reads `$EDITOR` from the environment, falls back to `"vi"`.
- For `BusType::System`: runs `sudo $EDITOR <fragment_path>`.
- For `BusType::Session`: runs `$EDITOR <fragment_path>`.
- Uses `.status()` for inherited stdio.
- Returns `Ok(())` on success or bails with the exit code.

---

## Related Files

| File | Role | Key Exports/Interfaces |
|------|------|----------------------|
| `src/systemd/commands.rs` | Defines `ServiceAction` enum and the blocking command execution functions | `ServiceAction`, `execute_systemctl()`, `edit_unit_file()` |
| `src/systemd/types.rs` | Defines `BusType` (System/Session) and `ServiceDetail` (contains `fragment_path`) | `BusType`, `ServiceDetail` |
| `src/app.rs` | Orchestrates the control flow: action requests, confirmation gating, suspend signal, and dispatching execution | `App::request_action()`, `App::request_action_for_unit()`, `App::execute_action()`, `App::execute_action_with_name()`, `App::edit_unit()`, `App::execute_suspended_action()`, `SuspendAction`, `InputMode::Confirm` |
| `src/ui/confirm.rs` | Renders the confirmation dialog overlay | `ConfirmDialog` (fields: `action`, `unit_name`; method: `render()`) |
| `src/ui/mod.rs` | Render dispatch: draws `ConfirmDialog` when `input_mode == Confirm` | `render()` function (overlay match arm at ~line 173) |
| `src/ui/context_menu.rs` | Defines `ContextMenuAction::ServiceAction` variant used by right-click menus | `ContextMenuAction`, `ContextMenuItem`, `ContextMenu` |
| `src/config/mod.rs` | Holds `ConfirmationsConfig` and `Config::needs_confirmation()` logic; loads from TOML | `Config`, `ConfirmationsConfig`, `Config::needs_confirmation()` |
| `src/config/keys.rs` | Maps key bindings to `KeyAction` variants including `Start`, `Stop`, `Restart`, `Enable`, `Disable`, `DaemonReload`, `EditUnit` | `KeyAction`, `KeyBindings` |
| `src/tui.rs` | Terminal lifecycle: init, restore, suspend, resume | `tui::init()`, `tui::restore()`, `tui::suspend()`, `tui::resume()` |
| `src/main.rs` | Main event loop: detects `needs_tui_suspend`, orchestrates suspend/execute/resume/reload cycle | `main()` (lines ~89-104) |

---

## Invariants and Constraints

1. **System services require sudo escalation.** Any `systemctl` command or editor invocation targeting `BusType::System` must be prefixed with `sudo`. Session-bus services use `systemctl --user` or the editor directly. This is unconditional and not configurable.

2. **TUI must be fully suspended before running external commands.** `tui::suspend()` must complete (leave alternate screen, disable raw mode, show cursor, disable mouse capture) before any `std::process::Command` is spawned. If the TUI is not suspended, `sudo` password prompts and editor UIs will be corrupted.

3. **TUI must be restored after the command completes.** `tui::resume()` must be called after the blocking command returns (whether it succeeded or failed). The resume creates a fresh `Terminal` handle that replaces the previous one. If resume is skipped, the application will not render.

4. **Units must be reloaded after any service action.** After every suspend/resume cycle, `load_units()` is called to re-fetch the full unit list from both D-Bus connections, followed by `apply_filters()` to update the sidebar. This ensures the displayed state matches reality.

5. **Confirmation dialogs are configurable per-action.** Each of the six service actions has an independent boolean in `ConfirmationsConfig`. A global toggle (`confirmations.global`) can disable all confirmations at once. All confirmations default to `true`. The config is loaded from `~/.config/sysdui/config.toml` under the `[confirmations]` section.

6. **`DaemonReload` does not take a unit name.** `ServiceAction::needs_unit()` returns `false` for `DaemonReload`. The unit name argument is not passed to `systemctl daemon-reload`.

7. **Edit requires a non-empty `fragment_path`.** If the selected unit's `ServiceDetail` has no `fragment_path` (empty string), `edit_unit()` returns early without setting `needs_tui_suspend`. This prevents launching an editor with no file argument.

8. **The `needs_tui_suspend` field is a one-shot signal.** It is set by action handlers in `App` and consumed (via `.take()`) by the main loop exactly once per cycle. Only one suspended action can be pending at a time.

9. **Error display blocks until user acknowledgment.** If the external command fails, the error is printed to stderr and the program waits for the user to press Enter before resuming the TUI. This ensures the user sees the error message.
