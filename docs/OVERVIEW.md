# sysdui Overview

## Description

sysdui is a terminal UI application for managing and monitoring systemd services. Built in Rust using ratatui for the TUI and zbus for D-Bus communication, it provides a split-pane interface for viewing service logs, managing services, and filtering/searching units.

## Subsystems

- **App (`src/app.rs`)**: Core application state machine. Holds the unit list, pane tree, input mode, and coordinates between subsystems.
- **UI (`src/ui/`)**: Rendering layer using ratatui. Composes sidebar, detail panel, log panes, help overlays, search bar, and context menus.
- **Pane System (`src/ui/panes.rs`)**: Binary tree data structure for split-pane log viewing. Handles layout computation, splitting, closing, and navigation.
- **Journal (`src/journal/`)**: Interfaces with journalctl for streaming log data into pane buffers. Includes priority filtering.
- **Systemd/D-Bus (`src/systemd/`)**: Communicates with systemd over D-Bus via zbus to list units, get details, and perform actions (start/stop/restart).
- **Event Handling (`src/event.rs`)**: Event loop bridging terminal input (crossterm), D-Bus updates, and application events.
- **Config (`src/config/`)**: Configuration loading (TOML-based) for keybindings, filters, and display preferences.
- **TUI (`src/tui.rs`)**: Terminal setup/teardown and frame rendering orchestration.

## Data Flow

1. **Startup**: `main.rs` loads config, initializes the terminal, connects to D-Bus, and enters the event loop.
2. **Event Loop**: `EventHandler` polls terminal events and D-Bus signals, dispatches `AppEvent` variants to `App`.
3. **User Input** -> `App` state transitions (input mode changes, pane splits, navigation) -> UI re-render.
4. **D-Bus** -> unit list updates, service state changes -> `App` state -> UI re-render.
5. **Journal** -> log lines stream into `PaneLeaf.log_buffer` via tokio tasks -> UI renders latest buffer contents.
6. **Layout**: `PaneTree::layout()` computes `Rect` positions for each leaf pane. The UI module iterates the layout to render each log pane.

## Features Index

### pane_layout
- **description**: Binary tree pane system with equal sizing for same-direction split chains.
- **entry_points**: `PaneTree::layout()`, `layout_node()`, `flatten_same_direction()`
- **depends_on**: []
- **doc**: docs/features/pane_layout.md
