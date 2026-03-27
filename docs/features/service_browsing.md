# Service Browsing

## Scope

### In Scope

- Loading systemd service units from D-Bus (system bus, session bus, or both)
- Displaying services in a scrollable sidebar list with status icons and selection highlighting
- Filtering by scope (User / System / Both) via `FilterMode`
- Filtering by active status (All / Active / Inactive / Failed) via `StatusFilter`
- Include/exclude list management via `ListMode` (All / Include / Exclude)
- Toggling individual services on/off the include and exclude lists, with immediate persistence to config
- Fuzzy search over unit names using `nucleo-matcher`
- Sorting by name, status, or uptime via `SortMode`
- Mouse interaction: click-to-select, double-click-to-open, scroll wheel navigation, right-click context menu, status bar zone clicking to cycle filters
- Keyboard navigation (up/down/wrapping) and filter cycling via configurable keybindings
- Live unit list updates via D-Bus signals (UnitNew, UnitRemoved, PropertiesChanged)
- Displaying current filter/sort state in the status line

### Out of Scope

- Service control actions (start, stop, restart, enable, disable, daemon-reload) -- these use the filtered list but are a separate feature
- Log viewing and journal streaming
- Service detail panel rendering
- Pane splitting and management
- Unit file editing

## Data / Control Flow

### 1. Initial Load

```
App::new()
  -> load_units()          -- queries D-Bus via dbus::list_units()
       -> system bus:  org.freedesktop.systemd1.Manager.ListUnits()
       -> session bus: org.freedesktop.systemd1.Manager.ListUnits()
       -> filters to services only: units.retain(|u| u.is_service())
       -> stores result in self.all_units: Vec<UnitInfo>
  -> apply_filters()       -- populates self.filtered_units from all_units
```

Initial filter/sort/list state is derived from `Config` (loaded from `~/.config/sysdui/config.toml`):
- `config.filter.show` -> `FilterMode` (user / system / both)
- `config.filter.status` -> `StatusFilter` (all / active / inactive / failed)
- `config.filter.mode` -> `ListMode` (all / include / exclude)
- `config.sort.default` -> `SortMode` (name / status / uptime)
- `config.filter.include` -> `Vec<String>` of unit names
- `config.filter.exclude` -> `Vec<String>` of unit names

### 2. apply_filters() Pipeline

`apply_filters()` is called after any change to filter state, sort mode, search query, or unit list. It processes the full `all_units` list through a sequential pipeline:

```
all_units.clone()
  |
  v
[1] FilterMode filter
    - User:   retain only bus_type == Session
    - System: retain only bus_type == System
    - Both:   no-op
  |
  v
[2] StatusFilter filter
    - All:      no-op
    - Active:   retain only active_state == Active
    - Inactive: retain only active_state == Inactive
    - Failed:   retain only active_state == Failed
  |
  v
[3] ListMode filter
    - All:     retain units NOT in exclude list (exclude list acts as a blocklist)
    - Include: retain only units IN include list (include list acts as an allowlist)
    - Exclude: retain only units IN exclude list (shows excluded units for management)
  |
  v
[4] Fuzzy search (if search_query is non-empty)
    - Creates nucleo_matcher::Atom with:
      - CaseMatching::Smart (case-insensitive unless query has uppercase)
      - Normalization::Smart
      - AtomKind::Fuzzy
    - Retains units where atom.score(unit.name) returns Some(_)
  |
  v
[5] Sort
    - Failed units always sort first (regardless of sort mode)
    - Then by SortMode:
      - Name:   alphabetical by unit name
      - Status: active units first, then alphabetical
      - Uptime: longest-running first (lowest start timestamp),
                units with ts=0 go last, ties broken by name
  |
  v
[6] Clamp selected_index
    - self.selected_index = min(self.selected_index, filtered_units.len() - 1)
    - If filtered_units is empty, selected_index = 0
```

The result is stored in `self.filtered_units: Vec<UnitInfo>`.

### 3. Sidebar Rendering

`sidebar::render_sidebar()` receives the filtered unit list and renders it as a `ratatui::widgets::List`:

- Each unit gets a status icon based on `active_state`:
  - Active: green filled circle
  - Failed: red X
  - Activating/Deactivating/Reloading: yellow target circle
  - Other (Inactive, etc.): gray empty circle
- A star marker is shown next to units that are in the include list (when not in Include mode)
- The selected item gets a highlight style (dark gray background, white bold text) with a triangle pointer symbol
- `ListState` is synchronized with `selected_index`
- The sidebar border is cyan when focused, dark gray otherwise

### 4. Filter Cycling via Keyboard

All filter/sort keys are configurable via `KeyBindings`. Default bindings and their actions:

| Default Key | KeyAction          | Effect                                                    |
|-------------|--------------------|------------------------------------------------------------|
| `f`         | CycleFilter        | Cycles FilterMode: User -> System -> Both -> User. Triggers `load_units()` (re-fetches from D-Bus) then `apply_filters()`. |
| `a`         | CycleStatusFilter  | Cycles StatusFilter: All -> Active -> Inactive -> Failed -> All. Triggers `apply_filters()`. |
| `i`         | ToggleListMode     | Cycles ListMode: All -> Include -> Exclude -> All. Triggers `apply_filters()`. |
| `t`         | CycleSort          | Cycles SortMode: Name -> Status -> Uptime -> Name. Triggers `apply_filters()`. |
| `/`         | SearchServices     | Enters SearchServices input mode. Clears search_query. Each keystroke appends to query and calls `apply_filters()`. Esc clears and exits. Enter exits keeping the filter. |

### 5. Include/Exclude List Editing

| Default Key | KeyAction      | Effect |
|-------------|----------------|--------|
| `+`         | ToggleInclude  | Toggles the currently selected unit on the include list. If already included, removes it. If not, adds it and removes from exclude (mutual exclusion). Calls `apply_filters()` then `save_filter_lists()`. |
| `-`         | ToggleExclude  | Toggles the currently selected unit on the exclude list. If already excluded, removes it. If not, adds it and removes from include (mutual exclusion). Calls `apply_filters()` then `save_filter_lists()`. |

`save_filter_lists()` persists both lists to `~/.config/sysdui/config.toml` by:
1. Reading the existing TOML file (or creating empty table)
2. Updating the `[filter].include` and `[filter].exclude` arrays
3. Writing back with `toml::to_string_pretty()`

### 6. Mouse Interaction

- **Left click on sidebar**: Sets `selected_index` to the clicked row (via `sidebar_row_to_index()` which accounts for border and scroll offset)
- **Double left click on sidebar**: Selects the unit and opens its detail (calls `select_service()`)
- **Right click on sidebar**: Opens a context menu for the clicked unit
- **Scroll wheel on sidebar**: Calls `navigate(-3)` / `navigate(3)` for scroll up/down
- **Left click on status bar**: The status bar is divided into 4 equal zones; clicking cycles the corresponding filter (zone 0 = scope, zone 1 = status, zone 2 = list mode, zone 3 = sort)

### 7. Live Updates via D-Bus Signals

- **UnitNew**: Re-fetches all units for that bus type, merges into `all_units` (removing old entries of the same bus type), then `apply_filters()`
- **UnitRemoved**: Removes the unit from `all_units` and `unit_details`, then `apply_filters()`
- **PropertiesChanged**: Re-fetches detail for the affected unit and updates `active_state` in `all_units` if it changed, then `apply_filters()`

### 8. Navigation

`navigate(delta)` moves `selected_index` by `delta` positions with wrapping via `rem_euclid(len)`. This means the list wraps from bottom to top and vice versa.

## Related Files

| File | Role | Key Exports / Interfaces |
|------|------|--------------------------|
| `src/app.rs` | Core application state and all filter/sort/search logic | `App` struct (`all_units`, `filtered_units`, `selected_index`, `filter_mode`, `status_filter`, `list_mode`, `sort_mode`, `search_query`), `apply_filters()`, `load_units()`, `navigate()`, `save_filter_lists()`, enums `FilterMode`, `StatusFilter`, `ListMode`, `SortMode`, `InputMode::SearchServices`, `HitTarget::Sidebar` |
| `src/ui/sidebar.rs` | Sidebar list rendering | `render_sidebar(frame, area, units, selected_index, focused, include_list, list_mode, state)` |
| `src/ui/mod.rs` | Top-level UI layout, status line rendering with filter labels, search bar integration | `render_ui()`, `LayoutCache` (stores `sidebar_area`, `status_line_area`, `sidebar_scroll_offset`) |
| `src/ui/search.rs` | Search bar widget | `SearchBar` widget |
| `src/ui/help.rs` | Help overlay listing keybindings for filter/sort actions | `render_help_bar()`, references `KeyAction::CycleFilter`, `CycleStatusFilter`, `ToggleInclude`, `ToggleExclude`, `CycleSort` |
| `src/config/mod.rs` | Configuration loading/saving, filter list persistence | `Config`, `FilterConfig` (mode, show, status, include, exclude), `SortConfig`, `load_config()`, `save_filter_lists()`, `config_path()` |
| `src/config/keys.rs` | Keybinding definitions and configuration | `KeyBindings`, `KeyAction` enum (CycleFilter, CycleStatusFilter, ToggleListMode, CycleSort, ToggleInclude, ToggleExclude, SearchServices), `apply_config_keys()` |
| `src/systemd/types.rs` | Unit data types | `UnitInfo` (name, active_state, bus_type, object_path, unit_kind, etc.), `ActiveState`, `BusType`, `UnitKind`, `ServiceDetail` (exec_main_start_timestamp for uptime sort) |
| `src/systemd/dbus.rs` | D-Bus communication for unit listing | `list_units(conn, bus_type) -> Vec<UnitInfo>`, `subscribe(conn)` for signal subscription |
| `src/event.rs` | Application event types | `AppEvent::UnitNew`, `AppEvent::UnitRemoved`, `AppEvent::PropertiesChanged`, `AppEvent::Tick` |

## Invariants and Constraints

1. **`filtered_units` is always a subset of `all_units`**: Every element in `filtered_units` originates from `all_units` via cloning and filtering. No unit can appear in `filtered_units` that is not in `all_units`.

2. **`apply_filters()` must be called after any filter/sort/search change**: Any mutation to `filter_mode`, `status_filter`, `list_mode`, `sort_mode`, `search_query`, or the contents of `all_units` / `config.filter.include` / `config.filter.exclude` must be followed by a call to `apply_filters()` to keep `filtered_units` consistent. Changing `filter_mode` additionally requires `load_units()` first because it affects which D-Bus buses are queried.

3. **`selected_index` must be clamped to `filtered_units.len()`**: After `apply_filters()`, `selected_index` is clamped to `filtered_units.len() - 1` (or 0 if empty). The `navigate()` function uses `rem_euclid` to wrap within bounds.

4. **Include/exclude lists are persisted immediately on change**: When `ToggleInclude` or `ToggleExclude` fires, `save_filter_lists()` is called synchronously after `apply_filters()`. Errors are logged as warnings but do not halt execution.

5. **Include and exclude lists are mutually exclusive per unit**: Adding a unit to the include list automatically removes it from the exclude list, and vice versa. This is enforced in the `ToggleInclude` and `ToggleExclude` handlers.

6. **Fuzzy search uses `nucleo-matcher` for scoring**: The search uses `Atom` with `AtomKind::Fuzzy`, `CaseMatching::Smart` (case-insensitive unless query contains uppercase), and `Normalization::Smart`. Units are retained if `atom.score()` returns `Some(_)` (any non-zero match).

7. **Failed units always sort first**: Regardless of the active `SortMode`, units with `active_state == Failed` are sorted before all other units. Secondary sorting follows the selected mode.

8. **`all_units` only contains services**: The `load_units()` method calls `units.retain(|u| u.is_service())` after fetching from D-Bus, filtering out timers, sockets, mounts, and all other unit types.

9. **FilterMode::User/System affects D-Bus queries**: Unlike other filters which operate on the in-memory `all_units`, changing `FilterMode` triggers a fresh `load_units()` call that only queries the relevant bus(es). This means switching from "Both" to "User" will discard system units from `all_units` entirely until the mode is changed back.

10. **ListMode::All uses the exclude list as a blocklist**: In `All` mode, the exclude list is still active -- units on it are hidden. This means the exclude list has an effect even when not in explicit "Exclude" view mode.
