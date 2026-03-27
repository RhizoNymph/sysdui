use std::collections::HashMap;
use std::time::Instant;

use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use nucleo_matcher::{
    Matcher,
    pattern::{Atom, AtomKind, CaseMatching, Normalization},
};
use ratatui::layout::{Position, Rect};
use tokio::sync::mpsc;
use zbus::Connection;

use crate::config::keys::KeyAction;
use crate::config::Config;
use crate::event::AppEvent;
use crate::journal;
use crate::journal::filter::Priority;
use crate::systemd::commands::{ServiceAction, edit_unit_file, execute_systemctl};
use crate::systemd::dbus;
use crate::systemd::types::*;
use crate::ui::LayoutCache;
use crate::ui::confirm::{ConfirmAction, ConfirmDialog};
use crate::ui::context_menu::{
    ContextMenu, ContextMenuAction, ContextMenuItem, ContextMenuTarget, compute_menu_rect,
};
use crate::ui::panes::{PaneId, PaneTree, SplitDirection};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    SearchServices,
    SearchLogs,
    Confirm,
    Help,
    SplitPrompt,
    ContextMenu,
}

pub enum HitTarget {
    Sidebar,
    Detail,
    Pane(PaneId),
    StatusBar,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMode {
    User,
    System,
    Both,
}

impl FilterMode {
    pub fn cycle_next(self) -> Self {
        match self {
            Self::User => Self::System,
            Self::System => Self::Both,
            Self::Both => Self::User,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::User => "User",
            Self::System => "System",
            Self::Both => "Both",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListMode {
    All,
    Include,
    Exclude,
}

impl ListMode {
    pub fn cycle_next(self) -> Self {
        match self {
            Self::All => Self::Include,
            Self::Include => Self::Exclude,
            Self::Exclude => Self::All,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Include => "Include",
            Self::Exclude => "Exclude",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusFilter {
    All,
    Active,
    Inactive,
    Failed,
}

impl StatusFilter {
    pub fn cycle_next(self) -> Self {
        match self {
            Self::All => Self::Active,
            Self::Active => Self::Inactive,
            Self::Inactive => Self::Failed,
            Self::Failed => Self::All,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Active => "Active",
            Self::Inactive => "Inactive",
            Self::Failed => "Failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    Name,
    Status,
    Uptime,
}

impl SortMode {
    pub fn cycle_next(self) -> Self {
        match self {
            Self::Name => Self::Status,
            Self::Status => Self::Uptime,
            Self::Uptime => Self::Name,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Status => "Status",
            Self::Uptime => "Uptime",
        }
    }
}

pub struct App {
    pub all_units: Vec<UnitInfo>,
    pub filtered_units: Vec<UnitInfo>,
    pub unit_details: HashMap<String, ServiceDetail>,
    pub selected_index: usize,
    pub filter_mode: FilterMode,
    pub status_filter: StatusFilter,
    pub list_mode: ListMode,
    pub sort_mode: SortMode,
    pub search_query: String,
    pub pane_tree: PaneTree,
    pub focused_pane: PaneId,
    pub input_mode: InputMode,
    pub confirm_dialog: Option<ConfirmDialog>,
    pub context_menu: Option<ContextMenu>,
    pub layout_cache: LayoutCache,
    pub last_click: Option<(u16, u16, Instant)>,
    pub config: Config,
    pub system_bus: Connection,
    pub session_bus: Connection,
    pub should_quit: bool,
    pub tx: mpsc::UnboundedSender<AppEvent>,
    pub needs_tui_suspend: Option<SuspendAction>,
}

pub enum SuspendAction {
    Systemctl {
        action: ServiceAction,
        unit_name: Option<String>,
        bus_type: BusType,
    },
    EditUnit {
        fragment_path: String,
        bus_type: BusType,
    },
}

impl App {
    pub async fn new(
        config: Config,
        system_bus: Connection,
        session_bus: Connection,
        tx: mpsc::UnboundedSender<AppEvent>,
    ) -> Result<Self> {
        // Compute config defaults
        let config_filter_mode = match config.filter.show.as_str() {
            "user" => FilterMode::User,
            "system" => FilterMode::System,
            _ => FilterMode::Both,
        };

        let config_list_mode = match config.filter.mode.as_str() {
            "include" => ListMode::Include,
            "exclude" => ListMode::Exclude,
            _ => ListMode::All,
        };

        let config_sort_mode = match config.sort.default.as_str() {
            "status" => SortMode::Status,
            "uptime" => SortMode::Uptime,
            _ => SortMode::Name,
        };

        let config_status_filter = match config.filter.status.as_str() {
            "active" => StatusFilter::Active,
            "inactive" => StatusFilter::Inactive,
            "failed" => StatusFilter::Failed,
            _ => StatusFilter::All,
        };

        let priority = config.log.priority;

        // Try to restore session state
        let session = match crate::state::load_session() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Failed to load session state: {e}");
                None
            }
        };

        let (filter_mode, status_filter, list_mode, sort_mode, pane_tree, focused_pane, selected_service) =
            if let Some(ref session) = session {
                let fm = match session.filter_mode.as_str() {
                    "user" => FilterMode::User,
                    "system" => FilterMode::System,
                    _ => FilterMode::Both,
                };
                let sf = match session.status_filter.as_str() {
                    "active" => StatusFilter::Active,
                    "inactive" => StatusFilter::Inactive,
                    "failed" => StatusFilter::Failed,
                    _ => StatusFilter::All,
                };
                let lm = match session.list_mode.as_str() {
                    "include" => ListMode::Include,
                    "exclude" => ListMode::Exclude,
                    _ => ListMode::All,
                };
                let sm = match session.sort_mode.as_str() {
                    "status" => SortMode::Status,
                    "uptime" => SortMode::Uptime,
                    _ => SortMode::Name,
                };
                let tree = session.to_pane_tree();
                (fm, sf, lm, sm, tree, session.focused_pane, session.selected_service.clone())
            } else {
                let tree = PaneTree::new(String::new(), priority);
                (config_filter_mode, config_status_filter, config_list_mode, config_sort_mode, tree, 1, None)
            };

        let mut app = Self {
            all_units: Vec::new(),
            filtered_units: Vec::new(),
            unit_details: HashMap::new(),
            selected_index: 0,
            filter_mode,
            status_filter,
            list_mode,
            sort_mode,
            search_query: String::new(),
            pane_tree,
            focused_pane,
            input_mode: InputMode::Normal,
            confirm_dialog: None,
            context_menu: None,
            layout_cache: LayoutCache::default(),
            last_click: None,
            config,
            system_bus,
            session_bus,
            should_quit: false,
            tx,
            needs_tui_suspend: None,
        };

        app.load_units().await?;
        app.apply_filters();

        // Restore selected service index from session
        if let Some(ref svc_name) = selected_service {
            if let Some(idx) = app.filtered_units.iter().position(|u| u.name == *svc_name) {
                app.selected_index = idx;
            }
        }

        // Start journal streams for all restored panes
        if session.is_some() {
            app.start_all_journal_streams();
        }

        Ok(app)
    }

    pub async fn load_units(&mut self) -> Result<()> {
        let mut units = Vec::new();

        if self.filter_mode != FilterMode::User {
            match dbus::list_units(&self.system_bus, BusType::System).await {
                Ok(mut system_units) => units.append(&mut system_units),
                Err(e) => tracing::warn!("Failed to list system units: {e}"),
            }
        }

        if self.filter_mode != FilterMode::System {
            match dbus::list_units(&self.session_bus, BusType::Session).await {
                Ok(mut user_units) => units.append(&mut user_units),
                Err(e) => tracing::warn!("Failed to list user units: {e}"),
            }
        }

        // Only keep services
        units.retain(|u| u.is_service());

        self.all_units = units;
        Ok(())
    }

    pub fn apply_filters(&mut self) {
        let mut filtered: Vec<UnitInfo> = self.all_units.clone();

        // Apply bus type filter
        match self.filter_mode {
            FilterMode::User => filtered.retain(|u| u.bus_type == BusType::Session),
            FilterMode::System => filtered.retain(|u| u.bus_type == BusType::System),
            FilterMode::Both => {}
        }

        // Apply status filter
        match self.status_filter {
            StatusFilter::All => {}
            StatusFilter::Active => {
                filtered.retain(|u| u.active_state == ActiveState::Active);
            }
            StatusFilter::Inactive => {
                filtered.retain(|u| u.active_state == ActiveState::Inactive);
            }
            StatusFilter::Failed => {
                filtered.retain(|u| u.active_state == ActiveState::Failed);
            }
        }

        // Apply include/exclude mode
        match self.list_mode {
            ListMode::Include => {
                filtered.retain(|u| self.config.filter.include.contains(&u.name));
            }
            ListMode::All => {
                filtered.retain(|u| !self.config.filter.exclude.contains(&u.name));
            }
            ListMode::Exclude => {
                filtered.retain(|u| self.config.filter.exclude.contains(&u.name));
            }
        }

        // Apply fuzzy search
        if !self.search_query.is_empty() {
            let mut matcher = Matcher::new(nucleo_matcher::Config::DEFAULT);
            let atom = Atom::new(
                &self.search_query,
                CaseMatching::Smart,
                Normalization::Smart,
                AtomKind::Fuzzy,
                false,
            );

            filtered.retain(|u| {
                let mut buf = Vec::new();
                let haystack = nucleo_matcher::Utf32Str::new(&u.name, &mut buf);
                atom.score(haystack, &mut matcher).is_some()
            });
        }

        // Sort
        filtered.sort_by(|a, b| {
            // Failed always first
            let a_failed = a.active_state == ActiveState::Failed;
            let b_failed = b.active_state == ActiveState::Failed;
            if a_failed != b_failed {
                return b_failed.cmp(&a_failed);
            }

            match self.sort_mode {
                SortMode::Name => a.name.cmp(&b.name),
                SortMode::Status => {
                    let a_active = a.active_state == ActiveState::Active;
                    let b_active = b.active_state == ActiveState::Active;
                    b_active.cmp(&a_active).then_with(|| a.name.cmp(&b.name))
                }
                SortMode::Uptime => {
                    // Sort by uptime descending (longest-running first).
                    // Lower start timestamp = earlier start = longer uptime.
                    let a_ts = self
                        .unit_details
                        .get(&a.name)
                        .map(|d| d.exec_main_start_timestamp)
                        .unwrap_or(0);
                    let b_ts = self
                        .unit_details
                        .get(&b.name)
                        .map(|d| d.exec_main_start_timestamp)
                        .unwrap_or(0);
                    // Earlier timestamp = longer uptime = should come first.
                    // Units with ts=0 (no start) go to the end.
                    match (a_ts, b_ts) {
                        (0, 0) => a.name.cmp(&b.name),
                        (0, _) => std::cmp::Ordering::Greater,
                        (_, 0) => std::cmp::Ordering::Less,
                        _ => a_ts.cmp(&b_ts).then_with(|| a.name.cmp(&b.name)),
                    }
                }
            }
        });

        self.filtered_units = filtered;

        // Clamp selected index
        if !self.filtered_units.is_empty() {
            self.selected_index = self.selected_index.min(self.filtered_units.len() - 1);
        } else {
            self.selected_index = 0;
        }
    }

    pub fn selected_unit(&self) -> Option<&UnitInfo> {
        self.filtered_units.get(self.selected_index)
    }

    pub fn selected_unit_name(&self) -> Option<String> {
        self.selected_unit().map(|u| u.name.clone())
    }

    pub async fn handle_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::Terminal(Event::Key(key)) => self.handle_key(key).await,
            AppEvent::Terminal(Event::Mouse(mouse)) => self.handle_mouse(mouse).await,
            AppEvent::Terminal(Event::Resize(_, _)) => {} // re-render handles this
            AppEvent::Tick => self.handle_tick().await,
            AppEvent::Render => {} // handled in main loop
            AppEvent::UnitNew {
                name, bus_type, ..
            } => {
                self.handle_unit_new(&name, bus_type).await;
            }
            AppEvent::UnitRemoved { name, .. } => {
                self.handle_unit_removed(&name);
            }
            AppEvent::PropertiesChanged {
                path,
                bus_type,
                changed,
            } => {
                self.handle_properties_changed(&path, bus_type, &changed)
                    .await;
            }
            AppEvent::LogLine { pane_id, line } => {
                self.handle_log_line(pane_id, line);
            }
            AppEvent::LogStreamEnded { pane_id } => {
                self.handle_log_stream_ended(pane_id);
            }
            AppEvent::CommandResult { action, result } => {
                tracing::info!("Command {action}: {result:?}");
            }
            _ => {}
        }
    }

    async fn handle_key(&mut self, key: KeyEvent) {
        // Ctrl-C always quits, regardless of input mode
        if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
            && key.code == KeyCode::Char('c')
        {
            self.should_quit = true;
            return;
        }

        match &self.input_mode {
            InputMode::SearchServices => {
                match key.code {
                    KeyCode::Esc => {
                        self.search_query.clear();
                        self.input_mode = InputMode::Normal;
                        self.apply_filters();
                    }
                    KeyCode::Enter => {
                        self.input_mode = InputMode::Normal;
                    }
                    KeyCode::Backspace => {
                        self.search_query.pop();
                        self.apply_filters();
                    }
                    KeyCode::Char(c) => {
                        self.search_query.push(c);
                        self.apply_filters();
                    }
                    _ => {}
                }
                return;
            }
            InputMode::SearchLogs => {
                match key.code {
                    KeyCode::Esc => {
                        if let Some(pane) = self.pane_tree.get_leaf_mut(self.focused_pane) {
                            pane.search_query.clear();
                        }
                        self.search_query.clear();
                        self.input_mode = InputMode::Normal;
                    }
                    KeyCode::Enter => {
                        if let Some(pane) = self.pane_tree.get_leaf_mut(self.focused_pane) {
                            pane.search_query = self.search_query.clone();
                        }
                        self.input_mode = InputMode::Normal;
                    }
                    KeyCode::Backspace => {
                        self.search_query.pop();
                    }
                    KeyCode::Char(c) => {
                        self.search_query.push(c);
                    }
                    _ => {}
                }
                return;
            }
            InputMode::Confirm => {
                match self.config.keys.get(&key) {
                    Some(KeyAction::Confirm) => {
                        if let Some(dialog) = self.confirm_dialog.take() {
                            match dialog.action {
                                ConfirmAction::ServiceAction { action, unit_name } => {
                                    self.execute_action_from_confirm(action, unit_name);
                                }
                                ConfirmAction::ResetState => {
                                    self.reset_state().await;
                                }
                            }
                        }
                        self.input_mode = InputMode::Normal;
                    }
                    _ => {
                        // Any other key cancels
                        self.confirm_dialog = None;
                        self.input_mode = InputMode::Normal;
                    }
                }
                return;
            }
            InputMode::Help => {
                // Any key closes help
                self.input_mode = InputMode::Normal;
                return;
            }
            InputMode::SplitPrompt => {
                match key.code {
                    KeyCode::Char('h') => {
                        self.split_pane(SplitDirection::Horizontal);
                        self.input_mode = InputMode::Normal;
                    }
                    KeyCode::Char('v') => {
                        self.split_pane(SplitDirection::Vertical);
                        self.input_mode = InputMode::Normal;
                    }
                    _ => {
                        self.input_mode = InputMode::Normal;
                    }
                }
                return;
            }
            InputMode::ContextMenu => {
                match key.code {
                    KeyCode::Down | KeyCode::Char('j') => {
                        if let Some(menu) = &mut self.context_menu {
                            menu.selected_index =
                                (menu.selected_index + 1).min(menu.items.len().saturating_sub(1));
                        }
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        if let Some(menu) = &mut self.context_menu {
                            menu.selected_index = menu.selected_index.saturating_sub(1);
                        }
                    }
                    KeyCode::Enter => {
                        self.execute_context_menu_action();
                    }
                    _ => {
                        self.context_menu = None;
                        self.input_mode = InputMode::Normal;
                    }
                }
                return;
            }
            InputMode::Normal => {}
        }

        // Normal mode key handling
        let Some(action) = self.config.keys.get(&key).copied() else {
            return;
        };

        match action {
            KeyAction::Quit => self.should_quit = true,
            KeyAction::NavigateUp => self.navigate(-1),
            KeyAction::NavigateDown => self.navigate(1),
            KeyAction::Select => self.select_service().await,
            KeyAction::SearchServices => {
                self.search_query.clear();
                self.input_mode = InputMode::SearchServices;
            }
            KeyAction::SearchLogs => {
                self.search_query.clear();
                self.input_mode = InputMode::SearchLogs;
            }
            KeyAction::CycleFilter => {
                self.filter_mode = self.filter_mode.cycle_next();
                self.load_units().await.ok();
                self.apply_filters();
                self.save_state();
            }
            KeyAction::CycleStatusFilter => {
                self.status_filter = self.status_filter.cycle_next();
                self.apply_filters();
                self.save_state();
            }
            KeyAction::ToggleListMode => {
                self.list_mode = self.list_mode.cycle_next();
                self.apply_filters();
                self.save_state();
            }
            KeyAction::ToggleInclude => {
                if let Some(name) = self.selected_unit_name() {
                    if let Some(pos) = self.config.filter.include.iter().position(|s| *s == name) {
                        self.config.filter.include.remove(pos);
                    } else {
                        self.config.filter.include.push(name.clone());
                        // Mutual exclusion: remove from exclude if present
                        self.config.filter.exclude.retain(|s| *s != name);
                    }
                    self.apply_filters();
                    self.save_filter_lists();
                }
            }
            KeyAction::ToggleExclude => {
                if let Some(name) = self.selected_unit_name() {
                    if let Some(pos) = self.config.filter.exclude.iter().position(|s| *s == name) {
                        self.config.filter.exclude.remove(pos);
                    } else {
                        self.config.filter.exclude.push(name.clone());
                        // Mutual exclusion: remove from include if present
                        self.config.filter.include.retain(|s| *s != name);
                    }
                    self.apply_filters();
                    self.save_filter_lists();
                }
            }
            KeyAction::CycleSort => {
                self.sort_mode = self.sort_mode.cycle_next();
                self.apply_filters();
                self.save_state();
            }
            KeyAction::CycleLogLevel => {
                // Extract needed values first to avoid borrow conflicts
                let info = self.pane_tree.get_leaf_mut(self.focused_pane).map(|pane| {
                    pane.priority_filter = pane.priority_filter.cycle_next();
                    let svc = pane.service_name.clone();
                    let priority = pane.priority_filter;
                    if let Some(h) = pane.journal_handle.take() {
                        h.abort();
                    }
                    pane.log_buffer.clear();
                    (svc, priority)
                });
                if let Some((svc, priority)) = info {
                    let bus_type = self.get_bus_type_for_service(&svc);
                    self.start_journal_for_pane(self.focused_pane, &svc, bus_type, priority);
                }
                self.save_state();
            }
            KeyAction::Start => self.request_action(ServiceAction::Start),
            KeyAction::Stop => self.request_action(ServiceAction::Stop),
            KeyAction::Restart => self.request_action(ServiceAction::Restart),
            KeyAction::Enable => self.request_action(ServiceAction::Enable),
            KeyAction::Disable => self.request_action(ServiceAction::Disable),
            KeyAction::DaemonReload => self.request_action(ServiceAction::DaemonReload),
            KeyAction::EditUnit => self.edit_unit(),
            KeyAction::PinPane => {
                self.input_mode = InputMode::SplitPrompt;
            }
            KeyAction::ClosePane => {
                let old_id = self.focused_pane;
                let next = self.pane_tree.next_leaf_id(old_id);
                if self.pane_tree.close(old_id) {
                    self.focused_pane = next;
                }
                self.save_state();
            }
            KeyAction::CycleFocus => {
                self.focused_pane = self.pane_tree.next_leaf_id(self.focused_pane);
                self.save_state();
            }
            KeyAction::ShowHelp => {
                self.input_mode = InputMode::Help;
            }
            KeyAction::ScrollUp => {
                if let Some(pane) = self.pane_tree.get_leaf_mut(self.focused_pane) {
                    pane.scroll_offset = pane.scroll_offset.saturating_add(1);
                }
            }
            KeyAction::ScrollDown => {
                if let Some(pane) = self.pane_tree.get_leaf_mut(self.focused_pane) {
                    pane.scroll_offset = pane.scroll_offset.saturating_sub(1);
                }
            }
            KeyAction::PageUp => {
                if let Some(pane) = self.pane_tree.get_leaf_mut(self.focused_pane) {
                    pane.scroll_offset = pane.scroll_offset.saturating_add(20);
                }
            }
            KeyAction::PageDown => {
                if let Some(pane) = self.pane_tree.get_leaf_mut(self.focused_pane) {
                    pane.scroll_offset = pane.scroll_offset.saturating_sub(20);
                }
            }
            KeyAction::Escape => {
                self.input_mode = InputMode::Normal;
            }
            KeyAction::ResetState => {
                self.confirm_dialog = Some(ConfirmDialog::new_reset());
                self.input_mode = InputMode::Confirm;
            }
            _ => {}
        }
    }

    fn hit_test(&self, col: u16, row: u16) -> HitTarget {
        let pos = Position { x: col, y: row };
        if self.layout_cache.sidebar_area.contains(pos) {
            return HitTarget::Sidebar;
        }
        if self.layout_cache.detail_area.contains(pos) {
            return HitTarget::Detail;
        }
        for (pane_id, rect) in &self.layout_cache.pane_rects {
            if rect.contains(pos) {
                return HitTarget::Pane(*pane_id);
            }
        }
        if self.layout_cache.status_line_area.contains(pos) {
            return HitTarget::StatusBar;
        }
        HitTarget::None
    }

    fn sidebar_row_to_index(&self, row: u16) -> Option<usize> {
        let sa = self.layout_cache.sidebar_area;
        let relative_row = row.saturating_sub(sa.y + 1) as usize; // -1 for top border
        let index = relative_row + self.layout_cache.sidebar_scroll_offset;
        if index < self.filtered_units.len() {
            Some(index)
        } else {
            None
        }
    }

    fn context_menu_rect(&self) -> Option<Rect> {
        let menu = self.context_menu.as_ref()?;
        let max_label_width = menu.items.iter().map(|i| i.label.len()).max().unwrap_or(0);
        Some(compute_menu_rect(
            menu.x,
            menu.y,
            menu.items.len(),
            max_label_width,
            self.layout_cache.frame_size,
        ))
    }

    async fn handle_mouse(&mut self, mouse: MouseEvent) {
        let col = mouse.column;
        let row = mouse.row;

        if self.input_mode == InputMode::ContextMenu {
            self.handle_mouse_context_menu(mouse);
            return;
        }

        match mouse.kind {
            MouseEventKind::ScrollUp => match self.hit_test(col, row) {
                HitTarget::Sidebar => self.navigate(-3),
                HitTarget::Pane(pane_id) => {
                    if let Some(pane) = self.pane_tree.get_leaf_mut(pane_id) {
                        pane.scroll_offset = pane.scroll_offset.saturating_add(3);
                    }
                }
                _ => {}
            },
            MouseEventKind::ScrollDown => match self.hit_test(col, row) {
                HitTarget::Sidebar => self.navigate(3),
                HitTarget::Pane(pane_id) => {
                    if let Some(pane) = self.pane_tree.get_leaf_mut(pane_id) {
                        pane.scroll_offset = pane.scroll_offset.saturating_sub(3);
                    }
                }
                _ => {}
            },
            MouseEventKind::Down(MouseButton::Left) => {
                // Double-click detection
                let is_double = if let Some((lx, ly, lt)) = self.last_click {
                    lx == col && ly == row && lt.elapsed().as_millis() < 300
                } else {
                    false
                };

                if is_double {
                    self.last_click = None;
                } else {
                    self.last_click = Some((col, row, Instant::now()));
                }

                match self.hit_test(col, row) {
                    HitTarget::Sidebar => {
                        if let Some(index) = self.sidebar_row_to_index(row) {
                            self.selected_index = index;
                            if is_double {
                                self.select_service().await;
                            }
                        }
                    }
                    HitTarget::Pane(pane_id) => {
                        self.focused_pane = pane_id;
                    }
                    HitTarget::StatusBar => {
                        let sl = self.layout_cache.status_line_area;
                        let zone_width = sl.width / 4;
                        if zone_width > 0 {
                            let relative_x = col.saturating_sub(sl.x);
                            let zone = (relative_x / zone_width).min(3);
                            match zone {
                                0 => {
                                    self.filter_mode = self.filter_mode.cycle_next();
                                    self.load_units().await.ok();
                                    self.apply_filters();
                                    self.save_state();
                                }
                                1 => {
                                    self.status_filter = self.status_filter.cycle_next();
                                    self.apply_filters();
                                    self.save_state();
                                }
                                2 => {
                                    self.list_mode = self.list_mode.cycle_next();
                                    self.apply_filters();
                                    self.save_state();
                                }
                                3 => {
                                    self.sort_mode = self.sort_mode.cycle_next();
                                    self.apply_filters();
                                    self.save_state();
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
            MouseEventKind::Down(MouseButton::Right) => match self.hit_test(col, row) {
                HitTarget::Sidebar => {
                    if let Some(index) = self.sidebar_row_to_index(row) {
                        let unit_name = self.filtered_units[index].name.clone();
                        self.open_sidebar_context_menu(col, row, unit_name);
                    }
                }
                HitTarget::Pane(pane_id) => {
                    self.open_pane_context_menu(col, row, pane_id);
                }
                _ => {}
            },
            MouseEventKind::Down(MouseButton::Middle) => {
                if let HitTarget::Pane(pane_id) = self.hit_test(col, row) {
                    let next = self.pane_tree.next_leaf_id(pane_id);
                    if self.pane_tree.close(pane_id) {
                        self.focused_pane = next;
                        self.save_state();
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_mouse_context_menu(&mut self, mouse: MouseEvent) {
        let col = mouse.column;
        let row = mouse.row;

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let menu_rect = self.context_menu_rect();
                if let Some(rect) = menu_rect {
                    if rect.contains(Position { x: col, y: row }) {
                        let item_row = row.saturating_sub(rect.y + 1) as usize;
                        let item_count = self
                            .context_menu
                            .as_ref()
                            .map(|m| m.items.len())
                            .unwrap_or(0);
                        if item_row < item_count {
                            if let Some(m) = &mut self.context_menu {
                                m.selected_index = item_row;
                            }
                            self.execute_context_menu_action();
                        }
                    } else {
                        self.context_menu = None;
                        self.input_mode = InputMode::Normal;
                    }
                } else {
                    self.context_menu = None;
                    self.input_mode = InputMode::Normal;
                }
            }
            MouseEventKind::ScrollUp => {
                if let Some(m) = &mut self.context_menu {
                    m.selected_index = m.selected_index.saturating_sub(1);
                }
            }
            MouseEventKind::ScrollDown => {
                if let Some(m) = &mut self.context_menu {
                    m.selected_index =
                        (m.selected_index + 1).min(m.items.len().saturating_sub(1));
                }
            }
            _ => {}
        }
    }

    fn open_sidebar_context_menu(&mut self, x: u16, y: u16, unit_name: String) {
        let items = vec![
            ContextMenuItem {
                label: "Start".into(),
                action: ContextMenuAction::ServiceAction(ServiceAction::Start),
            },
            ContextMenuItem {
                label: "Restart".into(),
                action: ContextMenuAction::ServiceAction(ServiceAction::Restart),
            },
            ContextMenuItem {
                label: "Stop".into(),
                action: ContextMenuAction::ServiceAction(ServiceAction::Stop),
            },
            ContextMenuItem {
                label: "Enable".into(),
                action: ContextMenuAction::ServiceAction(ServiceAction::Enable),
            },
            ContextMenuItem {
                label: "Disable".into(),
                action: ContextMenuAction::ServiceAction(ServiceAction::Disable),
            },
            ContextMenuItem {
                label: "Split Into Pane (H)".into(),
                action: ContextMenuAction::SplitNewPaneHorizontal,
            },
            ContextMenuItem {
                label: "Split Into Pane (V)".into(),
                action: ContextMenuAction::SplitNewPaneVertical,
            },
        ];
        self.context_menu = Some(ContextMenu {
            x,
            y,
            items,
            selected_index: 0,
            target: ContextMenuTarget::SidebarService { unit_name },
        });
        self.input_mode = InputMode::ContextMenu;
    }

    fn open_pane_context_menu(&mut self, x: u16, y: u16, pane_id: PaneId) {
        let items = vec![
            ContextMenuItem {
                label: "Split Horizontal".into(),
                action: ContextMenuAction::SplitHorizontal,
            },
            ContextMenuItem {
                label: "Split Vertical".into(),
                action: ContextMenuAction::SplitVertical,
            },
            ContextMenuItem {
                label: "Close Pane".into(),
                action: ContextMenuAction::ClosePane,
            },
        ];
        self.context_menu = Some(ContextMenu {
            x,
            y,
            items,
            selected_index: 0,
            target: ContextMenuTarget::Pane { pane_id },
        });
        self.input_mode = InputMode::ContextMenu;
    }

    fn execute_context_menu_action(&mut self) {
        let Some(menu) = self.context_menu.take() else {
            return;
        };
        self.input_mode = InputMode::Normal;

        let Some(item) = menu.items.get(menu.selected_index) else {
            return;
        };
        let action = item.action;

        match action {
            ContextMenuAction::ServiceAction(sa) => {
                if let ContextMenuTarget::SidebarService { unit_name } = menu.target {
                    self.request_action_for_unit(sa, unit_name);
                }
            }
            ContextMenuAction::SplitNewPaneHorizontal
            | ContextMenuAction::SplitNewPaneVertical => {
                if let ContextMenuTarget::SidebarService { unit_name } = menu.target {
                    let dir = if action == ContextMenuAction::SplitNewPaneHorizontal {
                        SplitDirection::Horizontal
                    } else {
                        SplitDirection::Vertical
                    };
                    let priority = self.config.log.priority;
                    if let Some(new_id) = self.pane_tree.split(
                        self.focused_pane,
                        dir,
                        unit_name.clone(),
                        priority,
                    ) {
                        self.focused_pane = new_id;
                        let bus_type = self.get_bus_type_for_service(&unit_name);
                        self.start_journal_for_pane(new_id, &unit_name, bus_type, priority);
                        self.save_state();
                    }
                }
            }
            ContextMenuAction::SplitHorizontal => {
                if let ContextMenuTarget::Pane { pane_id } = menu.target {
                    self.focused_pane = pane_id;
                    self.split_pane(SplitDirection::Horizontal);
                }
            }
            ContextMenuAction::SplitVertical => {
                if let ContextMenuTarget::Pane { pane_id } = menu.target {
                    self.focused_pane = pane_id;
                    self.split_pane(SplitDirection::Vertical);
                }
            }
            ContextMenuAction::ClosePane => {
                if let ContextMenuTarget::Pane { pane_id } = menu.target {
                    let next = self.pane_tree.next_leaf_id(pane_id);
                    if self.pane_tree.close(pane_id) {
                        self.focused_pane = next;
                        self.save_state();
                    }
                }
            }
        }
    }

    fn request_action_for_unit(&mut self, action: ServiceAction, unit_name: String) {
        if self.config.needs_confirmation(action.confirm_key()) {
            self.confirm_dialog = Some(ConfirmDialog::new_service(action, unit_name));
            self.input_mode = InputMode::Confirm;
        } else {
            self.execute_action_with_name(
                action,
                if action.needs_unit() {
                    Some(unit_name)
                } else {
                    None
                },
            );
        }
    }

    fn navigate(&mut self, delta: i32) {
        if self.filtered_units.is_empty() {
            return;
        }
        let len = self.filtered_units.len() as i32;
        let new_idx = (self.selected_index as i32 + delta).rem_euclid(len);
        self.selected_index = new_idx as usize;
    }

    async fn select_service(&mut self) {
        if let Some(unit) = self.selected_unit() {
            let name = unit.name.clone();
            let bus_type = unit.bus_type;
            let object_path = unit.object_path.clone();

            // Fetch detail if not cached
            if !self.unit_details.contains_key(&name) {
                let conn = match bus_type {
                    BusType::System => &self.system_bus,
                    BusType::Session => &self.session_bus,
                };
                match dbus::get_service_detail(conn, &object_path).await {
                    Ok(detail) => {
                        self.unit_details.insert(name.clone(), detail);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to get detail for {name}: {e}");
                    }
                }
            }

            // Update the focused pane's service
            let priority = self
                .pane_tree
                .get_leaf(self.focused_pane)
                .map(|p| p.priority_filter)
                .unwrap_or(self.config.log.priority);

            if let Some(pane) = self.pane_tree.get_leaf_mut(self.focused_pane) {
                if pane.service_name != name {
                    pane.service_name = name.clone();
                    pane.log_buffer.clear();
                    pane.scroll_offset = 0;
                    if let Some(h) = pane.journal_handle.take() {
                        h.abort();
                    }
                }
            }

            self.start_journal_for_pane(self.focused_pane, &name, bus_type, priority);
            self.save_state();
        }
    }

    fn start_journal_for_pane(
        &mut self,
        pane_id: PaneId,
        service_name: &str,
        bus_type: BusType,
        priority: Priority,
    ) {
        if service_name.is_empty() {
            return;
        }

        // Kill existing journal stream for this pane
        if let Some(pane) = self.pane_tree.get_leaf_mut(pane_id) {
            if let Some(h) = pane.journal_handle.take() {
                h.abort();
            }
        }

        let tx = self.tx.clone();
        match journal::spawn_journal_stream(service_name, bus_type, priority, pane_id, tx) {
            Ok(handle) => {
                if let Some(pane) = self.pane_tree.get_leaf_mut(pane_id) {
                    pane.journal_handle = Some(handle);
                }
            }
            Err(e) => {
                tracing::warn!("Failed to spawn journal stream: {e}");
            }
        }
    }

    fn get_bus_type_for_service(&self, name: &str) -> BusType {
        self.all_units
            .iter()
            .find(|u| u.name == name)
            .map(|u| u.bus_type)
            .unwrap_or(BusType::System)
    }

    fn request_action(&mut self, action: ServiceAction) {
        let unit_name = if action.needs_unit() {
            match self.selected_unit_name() {
                Some(name) => name,
                None => return,
            }
        } else {
            String::new()
        };

        if self.config.needs_confirmation(action.confirm_key()) {
            self.confirm_dialog = Some(ConfirmDialog::new_service(action, unit_name));
            self.input_mode = InputMode::Confirm;
        } else {
            self.execute_action_with_name(action, if action.needs_unit() { Some(unit_name) } else { None });
        }
    }

    fn execute_action_from_confirm(&mut self, action: ServiceAction, unit_name: String) {
        let unit_name_opt = if action.needs_unit() {
            Some(unit_name)
        } else {
            None
        };
        self.execute_action_with_name(action, unit_name_opt);
    }

    fn execute_action_with_name(&mut self, action: ServiceAction, unit_name: Option<String>) {
        let bus_type = unit_name
            .as_ref()
            .map(|n| self.get_bus_type_for_service(n))
            .unwrap_or(BusType::System);

        self.needs_tui_suspend = Some(SuspendAction::Systemctl {
            action,
            unit_name,
            bus_type,
        });
    }

    fn edit_unit(&mut self) {
        let Some(unit) = self.selected_unit() else {
            return;
        };
        let name = unit.name.clone();
        let bus_type = unit.bus_type;

        let fragment_path = self
            .unit_details
            .get(&name)
            .map(|d| d.fragment_path.clone())
            .unwrap_or_default();

        if fragment_path.is_empty() {
            return;
        }

        self.needs_tui_suspend = Some(SuspendAction::EditUnit {
            fragment_path,
            bus_type,
        });
    }

    fn save_filter_lists(&self) {
        if let Err(e) = crate::config::save_filter_lists(
            &self.config.filter.include,
            &self.config.filter.exclude,
        ) {
            tracing::warn!("Failed to save filter config: {e}");
        }
    }

    fn split_pane(&mut self, direction: SplitDirection) {
        let service_name = self.selected_unit_name().unwrap_or_default();
        let priority = self.config.log.priority;
        if let Some(new_id) = self.pane_tree.split(
            self.focused_pane,
            direction,
            service_name.clone(),
            priority,
        ) {
            self.focused_pane = new_id;
            if !service_name.is_empty() {
                let bus_type = self.get_bus_type_for_service(&service_name);
                self.start_journal_for_pane(new_id, &service_name, bus_type, priority);
            }
            self.save_state();
        }
    }

    fn handle_log_line(&mut self, pane_id: PaneId, line: String) {
        if let Some(pane) = self.pane_tree.get_leaf_mut(pane_id) {
            pane.push_line(line);
        }
    }

    fn handle_log_stream_ended(&mut self, pane_id: PaneId) {
        if let Some(pane) = self.pane_tree.get_leaf_mut(pane_id) {
            pane.journal_handle = None;
        }
    }

    async fn handle_unit_new(&mut self, name: &str, bus_type: BusType) {
        // Re-fetch units to pick up the new one
        let conn = match bus_type {
            BusType::System => &self.system_bus,
            BusType::Session => &self.session_bus,
        };
        if let Ok(units) = dbus::list_units(conn, bus_type).await {
            // Merge: remove old entries of same bus_type, add new ones
            self.all_units.retain(|u| u.bus_type != bus_type);
            self.all_units
                .extend(units.into_iter().filter(|u| u.is_service()));
            self.apply_filters();
        }
        tracing::debug!("Unit new: {name}");
    }

    fn handle_unit_removed(&mut self, name: &str) {
        self.all_units.retain(|u| u.name != name);
        self.unit_details.remove(name);
        self.apply_filters();
        tracing::debug!("Unit removed: {name}");
    }

    async fn handle_properties_changed(
        &mut self,
        path: &str,
        bus_type: BusType,
        _changed: &HashMap<String, zbus::zvariant::OwnedValue>,
    ) {
        // Find which unit this path belongs to
        let unit_name = self
            .all_units
            .iter()
            .find(|u| u.object_path == path)
            .map(|u| u.name.clone());

        if let Some(name) = unit_name {
            // Re-fetch detail for this unit
            let conn = match bus_type {
                BusType::System => &self.system_bus,
                BusType::Session => &self.session_bus,
            };
            match dbus::get_service_detail(conn, path).await {
                Ok(detail) => {
                    // Also update the unit's active_state in all_units
                    if let Some(unit) = self.all_units.iter_mut().find(|u| u.name == name) {
                        unit.active_state = ActiveState::from_str(&detail.active_state);
                        unit.sub_state = detail.sub_state.clone();
                    }
                    self.unit_details.insert(name, detail);
                    self.apply_filters();
                }
                Err(e) => {
                    tracing::debug!("Failed to refresh detail for path {path}: {e}");
                }
            }
        }
    }

    async fn handle_tick(&mut self) {
        // Fetch detail for currently selected unit if not cached
        if let Some(unit) = self.selected_unit() {
            let name = unit.name.clone();
            let object_path = unit.object_path.clone();
            let bus_type = unit.bus_type;

            if !self.unit_details.contains_key(&name) {
                let conn = match bus_type {
                    BusType::System => &self.system_bus,
                    BusType::Session => &self.session_bus,
                };
                if let Ok(detail) = dbus::get_service_detail(conn, &object_path).await {
                    self.unit_details.insert(name, detail);
                }
            }
        }
    }

    fn save_state(&self) {
        let state = crate::state::SessionState {
            filter_mode: self.filter_mode.label().to_lowercase(),
            status_filter: self.status_filter.label().to_lowercase(),
            list_mode: self.list_mode.label().to_lowercase(),
            sort_mode: self.sort_mode.label().to_lowercase(),
            selected_service: self.selected_unit_name(),
            focused_pane: self.focused_pane,
            next_pane_id: self.pane_tree.next_id,
            pane_tree: crate::state::SerializedPaneNode::from_pane_node(&self.pane_tree.root),
        };
        if let Err(e) = crate::state::save_session(&state) {
            tracing::warn!("Failed to save session state: {e}");
        }
    }

    fn start_all_journal_streams(&mut self) {
        let pane_info: Vec<_> = self
            .pane_tree
            .leaf_ids()
            .into_iter()
            .filter_map(|id| {
                self.pane_tree
                    .get_leaf(id)
                    .map(|p| (id, p.service_name.clone(), p.priority_filter))
            })
            .collect();

        for (id, svc, priority) in pane_info {
            if !svc.is_empty() {
                let bus_type = self.get_bus_type_for_service(&svc);
                self.start_journal_for_pane(id, &svc, bus_type, priority);
            }
        }
    }

    async fn reset_state(&mut self) {
        // Abort all journal handles
        for id in self.pane_tree.leaf_ids() {
            if let Some(pane) = self.pane_tree.get_leaf_mut(id) {
                if let Some(h) = pane.journal_handle.take() {
                    h.abort();
                }
            }
        }

        // Reset to defaults from config
        self.pane_tree = PaneTree::new(String::new(), self.config.log.priority);
        self.focused_pane = 1;
        self.filter_mode = match self.config.filter.show.as_str() {
            "user" => FilterMode::User,
            "system" => FilterMode::System,
            _ => FilterMode::Both,
        };
        self.status_filter = match self.config.filter.status.as_str() {
            "active" => StatusFilter::Active,
            "inactive" => StatusFilter::Inactive,
            "failed" => StatusFilter::Failed,
            _ => StatusFilter::All,
        };
        self.list_mode = match self.config.filter.mode.as_str() {
            "include" => ListMode::Include,
            "exclude" => ListMode::Exclude,
            _ => ListMode::All,
        };
        self.sort_mode = match self.config.sort.default.as_str() {
            "status" => SortMode::Status,
            "uptime" => SortMode::Uptime,
            _ => SortMode::Name,
        };
        self.search_query.clear();
        self.selected_index = 0;

        if let Err(e) = crate::state::delete_session() {
            tracing::warn!("Failed to delete session state: {e}");
        }

        self.load_units().await.ok();
        self.apply_filters();
    }

    /// Execute a suspended action (called from main loop after TUI is suspended).
    pub fn execute_suspended_action(action: &SuspendAction) -> Result<()> {
        match action {
            SuspendAction::Systemctl {
                action,
                unit_name,
                bus_type,
            } => {
                execute_systemctl(*action, unit_name.as_deref(), *bus_type)?;
            }
            SuspendAction::EditUnit {
                fragment_path,
                bus_type,
            } => {
                edit_unit_file(fragment_path, *bus_type)?;
            }
        }
        Ok(())
    }
}
