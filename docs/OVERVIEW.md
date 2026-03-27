# sysdui Overview

## Description

sysdui is a terminal UI application for managing systemd services, built in Rust using ratatui. It provides a single-screen interface to browse services, watch live logs, and control service lifecycle with features like fuzzy search, log tailing, pane splitting, and automatic privilege escalation.

## Subsystems

- **app** (`src/app.rs`): Central application state and event dispatch. Contains `App` struct, input mode handling, and all state mutation logic.
- **config** (`src/config/`): Configuration loading from TOML files and key binding management.
- **state** (`src/state.rs`): Session state persistence - saves/restores UI state across application restarts.
- **event** (`src/event.rs`): Event handling infrastructure - terminal events, D-Bus signals, timers.
- **journal** (`src/journal/`): Journal log streaming and filtering by priority level.
- **systemd** (`src/systemd/`): D-Bus communication with systemd, unit listing, service control commands.
- **tui** (`src/tui.rs`): Terminal initialization, suspend/resume for blocking operations.
- **ui** (`src/ui/`): Rendering layer - sidebar, detail pane, log panes, confirm dialogs, context menus, help overlay.

## Data Flow

1. `main.rs` sets up D-Bus connections, event handler, and creates `App`.
2. `EventHandler` multiplexes terminal input, D-Bus signals, and tick/render timers into `AppEvent`.
3. `App::handle_event()` dispatches events to key/mouse/tick/signal handlers that mutate `App` state.
4. On render events, `ui::render()` reads `App` state and draws the TUI.
5. State-modifying actions (filter changes, pane operations, service selection) trigger `save_state()` to persist session.
6. On startup, `App::new()` attempts to restore session state from disk before falling back to config defaults.

## Features Index

### session_state
- description: Persists UI session state (filters, sort, pane layout, selected service) to disk and restores on startup. Includes Ctrl-r hotkey to reset state to defaults.
- entry_points: [App::new, App::save_state, App::reset_state]
- depends_on: [config, panes]
- doc: docs/features/session_state.md

### pane_management
- description: Split-pane log viewing with horizontal/vertical splits, focus cycling, and close operations.
- entry_points: [App::split_pane, App::select_service, PaneTree]
- depends_on: [journal]
- doc: (not yet documented)

### service_control
- description: Start, stop, restart, enable, disable services and daemon-reload via systemctl with confirmation dialogs.
- entry_points: [App::request_action, App::execute_action_with_name]
- depends_on: [systemd]
- doc: (not yet documented)

### filtering_and_sorting
- description: Filter services by bus type, status, include/exclude lists, and fuzzy search. Sort by name, status, or uptime.
- entry_points: [App::apply_filters, App::navigate]
- depends_on: [config]
- doc: (not yet documented)

### key_bindings
- description: Configurable key bindings with defaults and TOML-based overrides.
- entry_points: [KeyBindings, apply_config_keys]
- depends_on: [config]
- doc: (not yet documented)
