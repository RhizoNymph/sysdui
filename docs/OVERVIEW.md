Overview:
    description:
        sysdui is a terminal-based UI for managing systemd services on Linux. It provides
        a modern, interactive interface to browse, monitor, and control systemd services
        (both system and user) with features like live log streaming, service status monitoring,
        fuzzy search, and multi-pane log viewing. Built in Rust using ratatui for rendering,
        crossterm for terminal events, and zbus for D-Bus communication with systemd.

    subsystems:
        App State Machine (src/app.rs):
            Central state machine that manages all application state and business logic.
            Processes events from the event system and coordinates between all other subsystems.
            Owns the pane tree, service list, input mode state machine, and filter/sort state.

        Event System (src/event.rs):
            Async event dispatch layer using tokio mpsc channels. Multiplexes terminal input,
            D-Bus signals, tick timers (~4Hz), render timers (~30Hz), journal log lines, and
            command results into a single AppEvent stream consumed by the App state machine.

        Systemd Module (src/systemd/):
            Interface to systemd via D-Bus (zbus). Handles listing units, subscribing to signals,
            querying service details, and executing systemctl commands. Supports both system and
            session buses.

        Journal Module (src/journal/):
            Manages live log streaming by spawning journalctl processes and forwarding output as
            AppEvents. Includes log filtering by priority level and text search matching.

        Config Module (src/config/):
            Loads and saves user configuration from ~/.config/sysdui/config.toml. Manages filter
            settings (include/exclude lists), keybindings, confirmation preferences, log defaults,
            and sort preferences. Only include/exclude lists are persisted back at runtime.

        UI Module (src/ui/):
            Rendering layer using ratatui. Manages layout computation, sidebar rendering, detail
            panel, log panes, search bar overlay, help screen, confirmation dialogs, context menus,
            and status bar. Produces a LayoutCache for mouse hit-testing.

        TUI Module (src/tui.rs):
            Low-level terminal initialization (raw mode, alternate screen, mouse capture),
            suspension for blocking operations (systemctl, editor), and restoration.

    data_flow:
        1. Initialization: main.rs sets up logging, loads config, connects to system and session
           D-Bus buses, subscribes to systemd signals, spawns event handlers (terminal reader,
           tick timer at ~4Hz, render timer at ~30Hz), initializes App state, and loads all units.

        2. Event Loop: All inputs converge into AppEvent via tokio mpsc channels. Terminal events
           (keyboard/mouse) come from crossterm, D-Bus signals (UnitNew, UnitRemoved,
           PropertiesChanged) come from zbus signal listeners, journal log lines come from spawned
           journalctl processes, and timers fire at fixed intervals.

        3. State Processing: App::handle_event() dispatches events to handle_key(), handle_mouse(),
           handle_tick(), or D-Bus signal handlers. These mutate App state (selected_index,
           filter/sort settings, pane tree, input mode, etc.) and call apply_filters() to
           recompute the visible service list.

        4. Rendering: On each Render event (~30Hz), ui::render() reads App state and draws the
           frame: sidebar (service list), detail panel (selected service info), log panes (binary
           tree layout), and overlays (search bar, help, confirm dialog, context menu).

        5. Blocking Operations: Service actions (start/stop/restart/etc.) and editor launch suspend
           the TUI (tui::suspend()), run the command in the foreground terminal, then resume
           (tui::resume()) and reload units.

Features Index:
    service_browsing:
        description: Browse, filter, sort, and fuzzy-search systemd services across system and user scopes
        entry_points: [src/app.rs:apply_filters, src/ui/sidebar.rs:render_sidebar]
        depends_on: [dbus_communication, configuration]
        doc: docs/features/service_browsing.md

    service_control:
        description: Start, stop, restart, enable, disable, daemon-reload services and edit unit files with privilege escalation
        entry_points: [src/app.rs:request_action, src/app.rs:execute_action, src/systemd/commands.rs:execute_systemctl]
        depends_on: [service_browsing, configuration]
        doc: docs/features/service_control.md

    log_streaming:
        description: Live-tail journalctl logs per service with priority filtering, text search, and scrollback
        entry_points: [src/journal/mod.rs:spawn_journal_stream, src/ui/logs.rs:render_log_pane]
        depends_on: [pane_management]
        doc: docs/features/log_streaming.md

    pane_management:
        description: Tmux-like binary tree pane splitting for viewing multiple service logs simultaneously
        entry_points: [src/ui/panes.rs:PaneTree, src/app.rs:split_pane]
        depends_on: [log_streaming]
        doc: docs/features/pane_management.md

    configuration:
        description: TOML-based user configuration for filters, keybindings, confirmations, log defaults, and sort preferences
        entry_points: [src/config/mod.rs:load_config, src/config/keys.rs:KeyBindings]
        depends_on: []
        doc: docs/features/configuration.md

    dbus_communication:
        description: D-Bus interface to systemd for listing units, subscribing to signals, and querying service details
        entry_points: [src/systemd/dbus.rs:list_units, src/systemd/dbus.rs:spawn_signal_listener]
        depends_on: []
        doc: docs/features/dbus_communication.md

    event_system:
        description: Async event dispatch multiplexing terminal input, D-Bus signals, timers, and journal output into a unified AppEvent stream
        entry_points: [src/event.rs:EventHandler, src/app.rs:handle_event]
        depends_on: []
        doc: docs/features/event_system.md

    ui_rendering:
        description: Terminal UI rendering with layout computation, sidebar, detail panel, log panes, and overlay widgets
        entry_points: [src/ui/mod.rs:render, src/ui/mod.rs:LayoutCache]
        depends_on: [service_browsing, log_streaming, pane_management]
        doc: docs/features/ui_rendering.md
