# Configuration

## Scope

### In Scope
- Loading a TOML configuration file from `~/.config/sysdui/config.toml`
- Filter settings: bus type scope (`mode`), show scope (`show`), status filter (`status`), include/exclude unit lists
- Keybinding customization: remapping any `KeyAction` to a different key combo via `[keys]` table
- Confirmation toggles: per-action and global control over whether confirmation dialogs appear before service actions
- Log defaults: default journal log priority level
- Sort defaults: default sort order for the unit list
- Persisting include/exclude list changes back to the config file at runtime

### Not In Scope
- Runtime application state (selection, pane layout, search queries) -- these are transient and not persisted
- D-Bus connection setup or systemd interaction -- handled by `systemd::dbus`
- Journal log reading -- handled by `journal` module; config only sets the initial priority

## Data/Control Flow

### Startup: `load_config()`

1. `main()` calls `config::load_config()`.
2. `load_config()` resolves the config path via `config_path()`, which returns `dirs::config_dir() / "sysdui" / "config.toml"` (typically `~/.config/sysdui/config.toml`).
3. If the file does not exist, `Config::default()` is returned immediately -- the config file is entirely optional.
4. If the file exists, the raw TOML is deserialized into `RawConfig` (all fields `Option`), then each present field is applied on top of `Config::default()`. This means any field omitted from the file retains its default value.
5. For `[keys]`, the raw `HashMap<String, String>` is passed to `keys::apply_config_keys()`, which maps action name strings to `KeyAction` variants and parses key combo strings via `parse_key_combo()`.
6. If `load_config()` fails (parse error, I/O error), `main()` prints a warning to stderr and falls back to `Config::default()`.
7. The resulting `Config` is passed into `App::new()`, which uses it to initialize `filter_mode`, `list_mode`, `sort_mode`, `status_filter`, and `log.priority`, then stores it as `app.config`.

```
main()
  |
  v
load_config() --> config_path() --> ~/.config/sysdui/config.toml
  |                                       |
  |  (file missing)                       | (file exists)
  v                                       v
Config::default()               read + parse RawConfig
                                       |
                                       v
                                overlay onto Config::default()
                                       |
                                       v
                                apply_config_keys()
                                       |
                                       v
                                    Config
  |
  v
App::new(config, ...) --> initializes app state from config fields
```

### Config Struct

`Config` is a flat struct with five sections:

| Field            | Type                 | Purpose                                         |
|------------------|----------------------|-------------------------------------------------|
| `confirmations`  | `ConfirmationsConfig`| Per-action confirmation dialog toggles           |
| `filter`         | `FilterConfig`       | Default filter/list settings and include/exclude |
| `log`            | `LogConfig`          | Default journal log priority                     |
| `sort`           | `SortConfig`         | Default sort order                               |
| `keys`           | `KeyBindings`        | Key-to-action mapping                            |

### FilterConfig

```
FilterConfig {
    mode: String,       // "all" | "include" | "exclude" -- which ListMode to start in
    show: String,       // "both" | "user" | "system" -- which bus types to show
    status: String,     // "all" | "active" | "inactive" | "failed" -- initial status filter
    include: Vec<String>, // unit names to include when in Include mode
    exclude: Vec<String>, // unit names to exclude when in All mode / show in Exclude mode
}
```

At startup, `App::new()` translates these strings into the corresponding enum variants (`FilterMode`, `ListMode`, `StatusFilter`). During runtime, `apply_filters()` in `App` reads `config.filter.include` and `config.filter.exclude` to filter the unit list based on the current `ListMode`:

- `ListMode::Include` -- only show units whose name is in `config.filter.include`
- `ListMode::All` -- show all units except those in `config.filter.exclude`
- `ListMode::Exclude` -- only show units whose name is in `config.filter.exclude`

### ConfirmationsConfig

```
ConfirmationsConfig {
    global: bool,        // master switch; if false, no confirmations at all
    start: bool,
    stop: bool,
    restart: bool,
    enable: bool,
    disable: bool,
    daemon_reload: bool,
}
```

All default to `true`. The `Config::needs_confirmation(action: &str) -> bool` method checks `global` first (short-circuits to `false` if disabled), then checks the per-action flag. This is called by `App::request_action_for_unit()` to decide whether to show a `ConfirmDialog` or execute the action immediately.

### KeyBindings

`KeyBindings` wraps a `HashMap<KeyEvent, KeyAction>`. It provides:

- `get(&KeyEvent) -> Option<&KeyAction>` -- look up the action for a pressed key
- `action_to_key(&KeyAction) -> Option<KeyEvent>` -- reverse lookup for rendering hints
- `all_bindings()` -- iterate all bindings

The default bindings are populated in `KeyBindings::default()`. Users override bindings in the `[keys]` TOML table using action name strings as keys and key combo strings as values (e.g., `start = "Ctrl-s"`).

`apply_config_keys()` processes user overrides by:
1. Building a lookup from action name string to `KeyAction` variant
2. For each user-specified mapping, removing the old binding for that action, then inserting the new key combo

`parse_key_combo()` handles combo syntax: modifier prefixes (`Ctrl-`, `Alt-`, `Shift-`) followed by a key name (`enter`, `esc`, `tab`, `up`, `f1`-`f12`, single characters, etc.).

`format_key_event()` converts a `KeyEvent` back to a human-readable string for the help bar and help overlay.

### Runtime Persistence: `save_filter_lists()`

Only the `include` and `exclude` lists are written back to disk during runtime. When the user toggles a unit into/out of the include or exclude list (`KeyAction::ToggleInclude` / `KeyAction::ToggleExclude`), `App::save_filter_lists()` calls `config::save_filter_lists()`, which:

1. Reads the existing config file (or creates an empty TOML table if the file doesn't exist)
2. Ensures a `[filter]` table exists
3. Overwrites the `include` and `exclude` arrays
4. Creates the parent directory if needed (`std::fs::create_dir_all`)
5. Writes the full TOML back via `toml::to_string_pretty`

This preserves all other config sections/values that may exist in the file.

### Usage Throughout the App

- **`src/main.rs`**: Calls `load_config()`, passes `Config` to `App::new()`
- **`src/app.rs`**: Stores `config` on `App`. Uses `config.filter.*` for include/exclude list filtering in `apply_filters()`. Uses `config.needs_confirmation()` in `request_action_for_unit()`. Uses `config.keys` to resolve `KeyEvent` to `KeyAction` in `handle_key()`. Uses `config.log.priority` when creating new panes. Calls `save_filter_lists()` after include/exclude changes.
- **`src/ui/help.rs`**: Uses `KeyBindings` to render the bottom help bar (`render_help_bar`) and the full-screen help overlay (`render_help_overlay`), both using `action_to_key()` and `format_key_event()` for display.

## Related Files

| File | Part of Feature | Key Exports/Interfaces |
|------|----------------|----------------------|
| `src/config/mod.rs` | Core config loading, structs, and persistence | `Config`, `ConfirmationsConfig`, `FilterConfig`, `LogConfig`, `SortConfig`, `load_config()`, `save_filter_lists()`, `config_path()` |
| `src/config/keys.rs` | Keybinding types, defaults, parsing, formatting | `KeyAction` (enum), `KeyBindings` (struct), `parse_key_combo()`, `format_key_event()`, `apply_config_keys()` |
| `src/main.rs` | Config loading entry point | Calls `load_config()`, passes result to `App::new()` |
| `src/app.rs` | Config consumption: filtering, confirmation checks, key dispatch | `App.config` field, `App::apply_filters()`, `App::request_action_for_unit()`, `App::handle_key()`, `App::save_filter_lists()` |
| `src/ui/help.rs` | Rendering key hints from bindings | `render_help_bar()`, `render_help_overlay()` |

### Full KeyAction Variants

| Variant | Default Key | Label | Config Name |
|---------|-------------|-------|-------------|
| `NavigateUp` | `k` / Up | "up" | `navigate_up` |
| `NavigateDown` | `j` / Down | "down" | `navigate_down` |
| `Select` | Enter | "select" | `select` |
| `SearchServices` | `/` | "search" | `search_services` |
| `SearchLogs` | Ctrl-`/` | "search logs" | `search_logs` |
| `EditUnit` | `e` | "edit" | `edit_unit` |
| `Start` | `s` | "start" | `start` |
| `Restart` | `r` | "restart" | `restart` |
| `Stop` | `x` | "stop" | `stop` |
| `Enable` | `n` | "enable" | `enable` |
| `Disable` | `d` | "disable" | `disable` |
| `DaemonReload` | `o` | "reload" | `daemon_reload` |
| `CycleFilter` | `f` | "scope" | `cycle_filter` |
| `CycleStatusFilter` | `a` | "status" | `cycle_status_filter` |
| `ToggleListMode` | `i` | "include" | `toggle_list_mode` |
| `PinPane` | `p` | "pin pane" | `pin_pane` |
| `ClosePane` | `w` | "close pane" | `close_pane` |
| `CycleFocus` | Tab | "focus" | `cycle_focus` |
| `CycleSort` | `t` | "sort" | `cycle_sort` |
| `CycleLogLevel` | `l` | "log level" | `cycle_log_level` |
| `ToggleInclude` | `+` | "+include" | `toggle_include` |
| `ToggleExclude` | `-` | "-exclude" | `toggle_exclude` |
| `Quit` | `q` | "quit" | `quit` |
| `ShowHelp` | `?` | "help" | `show_help` |
| `Confirm` | `y` | "confirm" | -- (not remappable) |
| `Cancel` | -- | "cancel" | -- (not remappable) |
| `ScrollUp` | -- | "scroll up" | -- (not remappable) |
| `ScrollDown` | -- | "scroll down" | -- (not remappable) |
| `PageUp` | PgUp | "page up" | -- (not remappable) |
| `PageDown` | PgDn | "page down" | -- (not remappable) |
| `SplitHorizontal` | -- | "split h" | -- (not remappable) |
| `SplitVertical` | -- | "split v" | -- (not remappable) |
| `Escape` | Esc | "escape" | -- (not remappable) |

Note: Actions marked "not remappable" are not present in the `action_map` inside `apply_config_keys()` and cannot be overridden via the config file.

## Invariants and Constraints

1. **Config file is optional.** If `~/.config/sysdui/config.toml` does not exist or fails to parse, the application uses `Config::default()` and continues normally. A warning is printed to stderr on parse failure.

2. **Config is loaded once at startup.** `load_config()` is called exactly once in `main()`. There is no hot-reload or file-watching mechanism. The `Config` struct is moved into `App` and remains for the lifetime of the process.

3. **Only filter include/exclude lists are written back at runtime.** `save_filter_lists()` is the sole write path. It surgically updates only the `[filter].include` and `[filter].exclude` arrays, preserving all other config content. No other config values are modified on disk during execution.

4. **KeyBindings default to a comprehensive set if not specified.** `KeyBindings::default()` populates all standard bindings. User-specified `[keys]` entries override individual actions; unmentioned actions retain their defaults. When an action is remapped, the old binding for that action is removed first to prevent duplicate mappings.

5. **No same-letter case-shifted keybindings.** The default keybinding set deliberately avoids pairing a lowercase letter with its uppercase counterpart for different actions (e.g., `i` and `I` are not both bound). All service action keys use distinct lowercase letters (`s`, `r`, `x`, `n`, `d`, `o`, `e`). This prevents accidental mis-invocations from caps-lock or shift being held.

6. **Partial configs are valid.** Every field in the TOML file is optional. The `RawConfig` and its nested structs use `Option` for all fields. Only fields present in the file override the defaults; omitted fields are silently ignored.

7. **ConfirmationsConfig global flag is a master switch.** When `confirmations.global` is `false`, `needs_confirmation()` returns `false` for all actions regardless of per-action settings. Per-action flags are only consulted when `global` is `true`.

8. **Key combo parsing is case-aware.** `parse_key_combo()` treats uppercase characters as `Shift + <lowercase>`. The `key()` helper in `keys.rs` similarly adds `KeyModifiers::SHIFT` for uppercase characters, ensuring consistent representation.
