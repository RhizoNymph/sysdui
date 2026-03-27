mod app;
mod config;
mod event;
mod journal;
mod state;
mod systemd;
mod tui;
mod ui;

use anyhow::Result;
use ratatui::widgets::ListState;
use tracing_subscriber::EnvFilter;

use app::App;
use config::load_config;
use event::{AppEvent, EventHandler};
use systemd::dbus;

#[tokio::main]
async fn main() -> Result<()> {
    // Set up file-based tracing
    let log_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("sysdui");
    std::fs::create_dir_all(&log_dir)?;
    let file_appender = tracing_appender::rolling::daily(&log_dir, "sysdui.log");
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("sysdui=info".parse()?))
        .with_writer(file_appender)
        .with_ansi(false)
        .init();

    tracing::info!("sysdui starting");

    // Load config
    let config = load_config().unwrap_or_else(|e| {
        eprintln!("Warning: failed to load config: {e}");
        config::Config::default()
    });

    // Connect to D-Bus
    let system_bus = dbus::system_bus().await?;
    let session_bus = dbus::session_bus().await?;

    // Subscribe to signals on both buses
    if let Err(e) = dbus::subscribe(&system_bus).await {
        tracing::warn!("Failed to subscribe to system bus signals: {e}");
    }
    if let Err(e) = dbus::subscribe(&session_bus).await {
        tracing::warn!("Failed to subscribe to session bus signals: {e}");
    }

    // Set up event handler
    let mut events = EventHandler::new();
    let tx = events.sender();

    events.spawn_terminal_reader();
    events.spawn_tick_timer();
    events.spawn_render_timer();

    // Spawn D-Bus signal listeners
    dbus::spawn_signal_listener(system_bus.clone(), systemd::types::BusType::System, tx.clone());
    dbus::spawn_signal_listener(
        session_bus.clone(),
        systemd::types::BusType::Session,
        tx.clone(),
    );

    // Initialize app
    let mut app = App::new(config, system_bus, session_bus, tx).await?;
    let mut sidebar_list_state = ListState::default();

    // Initialize terminal
    let mut terminal = tui::init()?;

    // Main event loop
    loop {
        let Some(event) = events.next().await else {
            break;
        };

        let is_render = matches!(event, AppEvent::Render);

        app.handle_event(event).await;

        if app.should_quit {
            break;
        }

        // Handle TUI suspend for blocking operations
        if let Some(action) = app.needs_tui_suspend.take() {
            tui::suspend()?;
            let result = App::execute_suspended_action(&action);
            if let Err(e) = &result {
                eprintln!("Error: {e}");
                eprintln!("Press Enter to continue...");
                let mut buf = String::new();
                let _ = std::io::stdin().read_line(&mut buf);
            }
            terminal = tui::resume()?;

            // Reload units after action
            let _ = app.load_units().await;
            app.apply_filters();
        }

        if is_render {
            let mut cache = None;
            terminal.draw(|frame| {
                cache = Some(ui::render(&app, &mut sidebar_list_state, frame));
            })?;
            if let Some(c) = cache {
                app.layout_cache = c;
            }
        }
    }

    tui::restore()?;
    tracing::info!("sysdui exiting");
    Ok(())
}
