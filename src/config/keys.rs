use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum KeyAction {
    NavigateUp,
    NavigateDown,
    Select,
    SearchServices,
    SearchLogs,
    EditUnit,
    Start,
    Restart,
    Stop,
    Enable,
    Disable,
    DaemonReload,
    CycleFilter,
    CycleStatusFilter,
    ToggleListMode,
    PinPane,
    ClosePane,
    CycleFocus,
    CycleSort,
    CycleLogLevel,
    ToggleInclude,
    ToggleExclude,
    ToggleDisabled,
    Quit,
    ShowHelp,
    Confirm,
    Cancel,
    ScrollUp,
    ScrollDown,
    PageUp,
    PageDown,
    SplitHorizontal,
    SplitVertical,
    Escape,
}

#[allow(dead_code)]
impl KeyAction {
    pub fn label(&self) -> &'static str {
        match self {
            Self::NavigateUp => "up",
            Self::NavigateDown => "down",
            Self::Select => "select",
            Self::SearchServices => "search",
            Self::SearchLogs => "search logs",
            Self::EditUnit => "edit",
            Self::Start => "start",
            Self::Restart => "restart",
            Self::Stop => "stop",
            Self::Enable => "enable",
            Self::Disable => "disable",
            Self::DaemonReload => "reload",
            Self::CycleFilter => "scope",
            Self::CycleStatusFilter => "status",
            Self::ToggleListMode => "include",
            Self::PinPane => "pin pane",
            Self::ClosePane => "close pane",
            Self::CycleFocus => "focus",
            Self::CycleSort => "sort",
            Self::CycleLogLevel => "log level",
            Self::ToggleInclude => "+include",
            Self::ToggleExclude => "-exclude",
            Self::ToggleDisabled => "disabled",
            Self::Quit => "quit",
            Self::ShowHelp => "help",
            Self::Confirm => "confirm",
            Self::Cancel => "cancel",
            Self::ScrollUp => "scroll up",
            Self::ScrollDown => "scroll down",
            Self::PageUp => "page up",
            Self::PageDown => "page down",
            Self::SplitHorizontal => "split h",
            Self::SplitVertical => "split v",
            Self::Escape => "escape",
        }
    }

    pub fn hint_key(&self, bindings: &KeyBindings) -> String {
        bindings
            .action_to_key(self)
            .map(format_key_event)
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub struct KeyBindings {
    map: HashMap<KeyEvent, KeyAction>,
}

impl KeyBindings {
    pub fn get(&self, key: &KeyEvent) -> Option<&KeyAction> {
        self.map.get(key)
    }

    pub fn action_to_key(&self, action: &KeyAction) -> Option<KeyEvent> {
        self.map
            .iter()
            .find(|(_, a)| *a == action)
            .map(|(k, _)| *k)
    }

    #[allow(dead_code)]
    pub fn all_bindings(&self) -> impl Iterator<Item = (&KeyEvent, &KeyAction)> {
        self.map.iter()
    }
}

impl Default for KeyBindings {
    fn default() -> Self {
        let mut map = HashMap::new();
        // Navigation
        map.insert(key('k'), KeyAction::NavigateUp);
        map.insert(key_code(KeyCode::Up), KeyAction::NavigateUp);
        map.insert(key('j'), KeyAction::NavigateDown);
        map.insert(key_code(KeyCode::Down), KeyAction::NavigateDown);
        map.insert(key_code(KeyCode::Enter), KeyAction::Select);
        map.insert(key_code(KeyCode::PageUp), KeyAction::PageUp);
        map.insert(key_code(KeyCode::PageDown), KeyAction::PageDown);
        // Search
        map.insert(key('/'), KeyAction::SearchServices);
        map.insert(
            KeyEvent::new(KeyCode::Char('/'), KeyModifiers::CONTROL),
            KeyAction::SearchLogs,
        );
        // Service actions — all distinct lowercase, no case pairs
        map.insert(key('e'), KeyAction::EditUnit);
        map.insert(key('s'), KeyAction::Start);
        map.insert(key('r'), KeyAction::Restart);
        map.insert(key('x'), KeyAction::Stop);
        map.insert(key('n'), KeyAction::Enable);
        map.insert(key('d'), KeyAction::Disable);
        map.insert(key('o'), KeyAction::DaemonReload);
        // Filtering / sorting
        map.insert(key('f'), KeyAction::CycleFilter);
        map.insert(key('a'), KeyAction::CycleStatusFilter);
        map.insert(key('i'), KeyAction::ToggleListMode);
        map.insert(key('t'), KeyAction::CycleSort);
        map.insert(key('l'), KeyAction::CycleLogLevel);
        map.insert(key('u'), KeyAction::ToggleDisabled);
        // Include/exclude list editing
        map.insert(key('+'), KeyAction::ToggleInclude);
        map.insert(key('-'), KeyAction::ToggleExclude);
        // Pane management
        map.insert(key('p'), KeyAction::PinPane);
        map.insert(key('w'), KeyAction::ClosePane);
        map.insert(key_code(KeyCode::Tab), KeyAction::CycleFocus);
        // General
        map.insert(key('q'), KeyAction::Quit);
        map.insert(key('?'), KeyAction::ShowHelp);
        map.insert(key('y'), KeyAction::Confirm);
        map.insert(key_code(KeyCode::Esc), KeyAction::Escape);
        Self { map }
    }
}

fn key(c: char) -> KeyEvent {
    if c.is_uppercase() {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::SHIFT)
    } else {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }
}

fn key_code(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

pub fn parse_key_combo(s: &str) -> Option<KeyEvent> {
    let s = s.trim();
    let parts: Vec<&str> = s.split('-').collect();
    if parts.is_empty() {
        return None;
    }

    let mut modifiers = KeyModifiers::NONE;
    let key_part = parts.last()?;

    for &part in &parts[..parts.len() - 1] {
        match part.to_lowercase().as_str() {
            "ctrl" | "c" => modifiers |= KeyModifiers::CONTROL,
            "alt" | "a" => modifiers |= KeyModifiers::ALT,
            "shift" | "s" => modifiers |= KeyModifiers::SHIFT,
            _ => return None,
        }
    }

    let code = match key_part.to_lowercase().as_str() {
        "enter" | "return" => KeyCode::Enter,
        "esc" | "escape" => KeyCode::Esc,
        "tab" => KeyCode::Tab,
        "backspace" | "bs" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "space" | " " => KeyCode::Char(' '),
        s if s.len() == 1 => {
            let c = s.chars().next()?;
            if modifiers.contains(KeyModifiers::SHIFT) && c.is_alphabetic() {
                KeyCode::Char(c.to_uppercase().next()?)
            } else {
                KeyCode::Char(c)
            }
        }
        s if s.starts_with('f') => {
            let n: u8 = s[1..].parse().ok()?;
            KeyCode::F(n)
        }
        _ => return None,
    };

    // If the char is uppercase, add SHIFT modifier
    if let KeyCode::Char(c) = code {
        if c.is_uppercase() && !modifiers.contains(KeyModifiers::SHIFT) {
            modifiers |= KeyModifiers::SHIFT;
        }
    }

    Some(KeyEvent::new(code, modifiers))
}

pub fn format_key_event(key: KeyEvent) -> String {
    let mut parts = Vec::new();
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("Ctrl".to_string());
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        parts.push("Alt".to_string());
    }

    let key_str = match key.code {
        KeyCode::Char(' ') => "Space".to_string(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Esc => "Esc".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::Backspace => "BS".to_string(),
        KeyCode::Delete => "Del".to_string(),
        KeyCode::Up => "↑".to_string(),
        KeyCode::Down => "↓".to_string(),
        KeyCode::Left => "←".to_string(),
        KeyCode::Right => "→".to_string(),
        KeyCode::PageUp => "PgUp".to_string(),
        KeyCode::PageDown => "PgDn".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        KeyCode::F(n) => format!("F{n}"),
        _ => "?".to_string(),
    };
    parts.push(key_str);
    parts.join("-")
}

pub fn apply_config_keys(
    bindings: &mut KeyBindings,
    keys: &std::collections::HashMap<String, String>,
) {
    let action_map: HashMap<&str, KeyAction> = HashMap::from([
        ("navigate_up", KeyAction::NavigateUp),
        ("navigate_down", KeyAction::NavigateDown),
        ("select", KeyAction::Select),
        ("search_services", KeyAction::SearchServices),
        ("search_logs", KeyAction::SearchLogs),
        ("edit_unit", KeyAction::EditUnit),
        ("start", KeyAction::Start),
        ("restart", KeyAction::Restart),
        ("stop", KeyAction::Stop),
        ("enable", KeyAction::Enable),
        ("disable", KeyAction::Disable),
        ("daemon_reload", KeyAction::DaemonReload),
        ("cycle_filter", KeyAction::CycleFilter),
        ("cycle_status_filter", KeyAction::CycleStatusFilter),
        ("toggle_list_mode", KeyAction::ToggleListMode),
        ("toggle_include", KeyAction::ToggleInclude),
        ("toggle_exclude", KeyAction::ToggleExclude),
        ("toggle_disabled", KeyAction::ToggleDisabled),
        ("pin_pane", KeyAction::PinPane),
        ("close_pane", KeyAction::ClosePane),
        ("cycle_focus", KeyAction::CycleFocus),
        ("cycle_sort", KeyAction::CycleSort),
        ("cycle_log_level", KeyAction::CycleLogLevel),
        ("quit", KeyAction::Quit),
        ("show_help", KeyAction::ShowHelp),
    ]);

    for (name, combo_str) in keys {
        if let Some(&action) = action_map.get(name.as_str()) {
            if let Some(key_event) = parse_key_combo(combo_str) {
                // Remove old binding for this action
                bindings.map.retain(|_, a| *a != action);
                bindings.map.insert(key_event, action);
            }
        }
    }
}
