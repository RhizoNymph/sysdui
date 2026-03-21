use std::collections::HashMap;

use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use nucleo_matcher::{
    Matcher,
    pattern::{Atom, AtomKind, CaseMatching, Normalization},
};
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
use crate::ui::confirm::ConfirmDialog;
use crate::ui::panes::{PaneId, PaneTree, SplitDirection};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    SearchServices,
    SearchLogs,
    Confirm,
    Help,
    SplitPrompt,
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
        let filter_mode = match config.filter.show.as_str() {
            "user" => FilterMode::User,
            "system" => FilterMode::System,
            _ => FilterMode::Both,
        };

        let list_mode = match config.filter.mode.as_str() {
            "include" => ListMode::Include,
            "exclude" => ListMode::Exclude,
            _ => ListMode::All,
        };

        let sort_mode = match config.sort.default.as_str() {
            "status" => SortMode::Status,
            "uptime" => SortMode::Uptime,
            _ => SortMode::Name,
        };

        let status_filter = match config.filter.status.as_str() {
            "active" => StatusFilter::Active,
            "inactive" => StatusFilter::Inactive,
            "failed" => StatusFilter::Failed,
            _ => StatusFilter::All,
        };

        let priority = config.log.priority;

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
            pane_tree: PaneTree::new(String::new(), priority),
            focused_pane: 1,
            input_mode: InputMode::Normal,
            confirm_dialog: None,
            config,
            system_bus,
            session_bus,
            should_quit: false,
            tx,
            needs_tui_suspend: None,
        };

        app.load_units().await?;
        app.apply_filters();

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
            AppEvent::Terminal(Event::Mouse(mouse)) => self.handle_mouse(mouse),
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
                            self.execute_action(dialog);
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
            }
            KeyAction::CycleStatusFilter => {
                self.status_filter = self.status_filter.cycle_next();
                self.apply_filters();
            }
            KeyAction::ToggleListMode => {
                self.list_mode = self.list_mode.cycle_next();
                self.apply_filters();
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
            }
            KeyAction::CycleFocus => {
                self.focused_pane = self.pane_tree.next_leaf_id(self.focused_pane);
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
            _ => {}
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                if let Some(pane) = self.pane_tree.get_leaf_mut(self.focused_pane) {
                    pane.scroll_offset = pane.scroll_offset.saturating_add(3);
                }
            }
            MouseEventKind::ScrollDown => {
                if let Some(pane) = self.pane_tree.get_leaf_mut(self.focused_pane) {
                    pane.scroll_offset = pane.scroll_offset.saturating_sub(3);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                // Check if click is in sidebar area (rough heuristic: x < 35)
                if mouse.column < 35 {
                    // Sidebar click — calculate which row
                    let row = mouse.row.saturating_sub(1) as usize; // account for border
                    if row < self.filtered_units.len() {
                        self.selected_index = row;
                        let tx = self.tx.clone();
                        let _ = tx; // selection will trigger detail fetch on next tick
                    }
                }
            }
            _ => {}
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
            self.confirm_dialog = Some(ConfirmDialog::new(action, unit_name));
            self.input_mode = InputMode::Confirm;
        } else {
            self.execute_action_with_name(action, if action.needs_unit() { Some(unit_name) } else { None });
        }
    }

    fn execute_action(&mut self, dialog: ConfirmDialog) {
        let unit_name = if dialog.action.needs_unit() {
            Some(dialog.unit_name)
        } else {
            None
        };
        self.execute_action_with_name(dialog.action, unit_name);
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
