# UI Rendering

## Scope

### In Scope

- **Layout computation**: Top-level frame partitioning into sidebar, detail panel, log panes, status bar, and help bar regions. Conditional search bar insertion.
- **Sidebar rendering**: Service list with status icons, star markers, selection highlighting, scroll state, and focus borders.
- **Detail panel rendering**: Selected service metadata display (name, status, PID, memory, uptime, unit file path, enabled state, dependencies, description).
- **Log pane rendering**: Log line display with scroll position, scrollbar, LIVE/+N indicators, search match highlighting, and focus-dependent border coloring.
- **Search bar overlay**: Inline search input widget with cursor rendering and placeholder text, displayed at top of content area.
- **Help screen**: Full-screen centered overlay listing all keybindings grouped by category.
- **Help bar**: Bottom-row context-sensitive keybinding hints with configurable key labels.
- **Confirmation dialog**: Centered modal dialog for confirming destructive service actions.
- **Context menu**: Position-aware right-click menu for sidebar services and log panes.
- **Split prompt**: Small centered dialog prompting for horizontal/vertical split direction.
- **Status bar**: Single-line display of current filter scope, status filter, list mode (with count), and sort mode.
- **LayoutCache**: Struct that captures computed rectangles from the render pass for use by the mouse hit-testing system in `App`.

### Not In Scope

- **Business logic**: All state mutations, event handling, and service action execution live in `src/app.rs`.
- **Event handling**: Keyboard/mouse input dispatch is handled by `App::handle_event()`, `handle_key()`, `handle_mouse()`.
- **Pane tree structure and manipulation**: `PaneTree` splitting, closing, and traversal logic lives in `src/ui/panes.rs` and is a data structure concern, not a rendering concern. The rendering code only calls `pane_tree.layout()` and `pane_tree.get_leaf()`.
- **Journal streaming**: Log data arrives via channels managed by `src/journal/mod.rs`.
- **D-Bus communication**: Unit info and service details are fetched by `src/systemd/`.
- **Configuration loading**: Config is loaded by `src/config/mod.rs`; the UI only reads `app.config`.

---

## Data/Control Flow

### Entry Point

The single entry point is `ui::render(app: &App, sidebar_state: &mut ListState, frame: &mut Frame) -> LayoutCache`, called from the main event loop in `src/main.rs` at ~30Hz inside `terminal.draw()`. The returned `LayoutCache` is stored on `App` for subsequent mouse hit-testing.

### Frame Orchestration

`render()` builds the full frame in a fixed sequence of layout splits and widget renders:

```
+---------------------------------------------------------------+
|  [Search Bar - 3 lines, only if InputMode::SearchServices      |
|   or InputMode::SearchLogs]                                    |
+-------------------+-------------------------------------------+
|                   |  Detail Panel (fixed 9 lines)              |
|  Sidebar          +-------------------------------------------+
|  (35 chars or     |                                            |
|   1/3 width,      |  Log Panes (remaining height)              |
|   whichever is    |  (binary tree layout from PaneTree)        |
|   smaller)        |                                            |
+-------------------+-------------------------------------------+
|  Status Line (1 line)                                          |
+---------------------------------------------------------------+
|  Help Bar (1 line)                                             |
+---------------------------------------------------------------+
```

#### Step 1: Vertical Split - Content vs Bottom Bars

The frame is split vertically into:
- **Main content area** (`Constraint::Min(1)`) - everything above the bottom bars
- **Bottom bar area** (`Constraint::Length(2)`) - status line + help bar

#### Step 2: Conditional Search Bar

If `app.input_mode` is `SearchServices` or `SearchLogs`, the main content area is further split:
- **Search bar** (`Constraint::Length(3)`) - rendered at the top
- **Remaining content** (`Constraint::Min(1)`)

Otherwise, the full main content area is used for the remaining layout.

#### Step 3: Horizontal Split - Sidebar vs Main Panel

The content area (after optional search bar removal) is split horizontally:
- **Sidebar** (`Constraint::Length(sidebar_width)`) where `sidebar_width = min(35, content_width / 3)`
- **Main panel** (`Constraint::Min(1)`)

#### Step 4: Main Panel Vertical Split - Detail vs Logs

The main panel is split vertically:
- **Detail panel** (`Constraint::Length(9)`) - fixed height of 9 rows
- **Log area** (`Constraint::Min(1)`) - remaining vertical space

#### Step 5: Log Pane Layout

`app.pane_tree.layout(log_area)` computes a `Vec<(PaneId, Rect)>` by recursively splitting the log area according to the binary tree of `PaneSplit` nodes. Each split divides space by its `ratio` (default 0.5) either horizontally or vertically.

#### Step 6: Widget Rendering Order

1. **Search bar** (if active) - `search::SearchBar` widget
2. **Sidebar** - `sidebar::render_sidebar()`
3. **Detail panel** - `detail::render_detail()`
4. **Log panes** - `logs::render_log_pane()` for each leaf in the pane tree
5. **Status line** - inline `Paragraph` with filter/sort state spans
6. **Help bar** - `help::render_help_bar()`
7. **Overlays** (rendered last, on top of all content):
   - `InputMode::Confirm` -> `ConfirmDialog::render()` (centered, clears background)
   - `InputMode::Help` -> `help::render_help_overlay()` (centered, clears background)
   - `InputMode::SplitPrompt` -> `render_split_prompt()` (centered, clears background)
   - `InputMode::ContextMenu` -> `context_menu::render_context_menu()` (positioned at click coords, clears background)

### LayoutCache

After rendering, `render()` constructs and returns a `LayoutCache` containing:
- `sidebar_area: Rect` - the sidebar rectangle
- `detail_area: Rect` - the detail panel rectangle
- `pane_rects: Vec<(PaneId, Rect)>` - all log pane rectangles with their IDs
- `status_line_area: Rect` - the status bar rectangle
- `sidebar_scroll_offset: usize` - current scroll offset from `ListState` (for mapping click row to unit index)
- `frame_size: Rect` - full terminal size

The main loop stores this on `App::layout_cache` after each draw. It is consumed by:
- `App::hit_test(col, row) -> HitTarget` - determines which UI region a mouse click lands in
- `App::sidebar_row_to_index(row)` - converts a click row to a unit list index, accounting for scroll offset and top border
- `App::context_menu_rect()` - computes context menu bounds for click-outside-to-dismiss detection

### Sidebar Rendering

`sidebar::render_sidebar()` renders a `ratatui::widgets::List` of service items.

**Status icons** (colored by `ActiveState`):
| Icon | Color | State |
|------|-------|-------|
| `●` | Green | Active |
| `✗` | Red | Failed |
| `◎` | Yellow | Activating, Deactivating, Reloading |
| `○` | DarkGray | Inactive and all other states |

**Star markers**: When `list_mode != ListMode::Include`, services that appear in `config.filter.include` are prefixed with `★`. In Include mode, the star is suppressed since all visible services are included.

**Selection**: The currently selected item uses `highlight_style` (white text on dark gray background, bold) and the `▶ ` highlight symbol.

**Focus**: The border is Cyan when focused (`InputMode::Normal`), DarkGray otherwise.

### Detail Panel Rendering

`detail::render_detail()` renders a `Paragraph` inside a bordered block titled " Detail ". The block always uses DarkGray borders (it is not focusable).

When a service is selected and its `ServiceDetail` is loaded, the panel shows:
1. **Service name** - white, bold
2. **Status** - `active_state (sub_state)`, colored by state (Green/Red/Yellow/DarkGray) + PID if non-zero
3. **Memory + Uptime** - `memory_human()` and `uptime_human()` formatted values
4. **Unit file path** (`fragment_path`) - if non-empty
5. **Enabled state** (`unit_file_state`) - if non-empty
6. **Dependencies** - first 5 entries from `requires` + `wants`, with `...` suffix if truncated
7. **Description** - italic, DarkGray, separated by a blank line

If no detail is loaded yet, shows "Loading...". If no service is selected, shows "No service selected".

### Log Pane Rendering

`logs::render_log_pane()` renders each log pane with:

**Title**: `" Logs: {service_name} [{priority_filter}] "`

**Bottom title (right-aligned)**:
- `" [LIVE] "` when `scroll_offset == 0` (following/tail mode)
- `" [+N new] "` when scrolled back, showing the scroll offset

**Border color**: Cyan if focused, DarkGray if unfocused.

**Log content**: Displays lines from `pane.log_buffer` (a `VecDeque<String>` capped at 10,000 lines). The visible window is computed based on:
- `visible_height` = inner area height
- When following (`scroll_offset == 0`): shows the last `visible_height` lines
- When scrolled back: shows lines starting from `total_lines - visible_height - scroll_offset`

**Search highlighting**: When `pane.search_query` is non-empty, matching substrings are highlighted with yellow background and black foreground via `journal::filter::find_matches()`.

**Scrollbar**: A vertical scrollbar is rendered on the right side (inside the border margin) when `total_lines > visible_height`, using `ratatui::widgets::Scrollbar`.

### Search Bar

`search::SearchBar` is a `Widget` implementation that renders:
- A bordered input field with yellow borders and a context-sensitive title (` Search Services ` or ` Search Logs `)
- The current query text, or `"type to search..."` placeholder in DarkGray if empty
- A block cursor at the current position (white background, black foreground)

### Help Overlay

`help::render_help_overlay()` renders a centered overlay (up to 60x30, clamped to terminal size minus 4) containing all keybindings organized into categories:
- Navigation (up, down, select, page up/down)
- Service Actions (start, restart, stop, enable, disable, reload, edit)
- Search & Filter (search services/logs, cycle filter/status/mode/sort, toggle include/exclude, cycle log level)
- Panes (pin/split, close, cycle focus)
- General (quit, help, escape)

Each entry shows the configured key (from `KeyBindings`) formatted via `format_key_event()` and the action's label. The overlay clears the background area and has Cyan borders with the title `" Keybindings (press any key to close) "`.

### Help Bar

`help::render_help_bar()` renders a single-line bar at the very bottom. In search mode, it shows `Enter:confirm Esc:cancel`. In normal mode, it shows two groups separated by ` | `:
- **Service actions**: start, restart, stop, enable, disable, reload, edit
- **View & panes**: search, scope, status, mode, +incl, -excl, sort, log, split, close, focus, help

Each hint displays the configured keybinding in yellow brackets followed by the label.

### Confirmation Dialog

`ConfirmDialog::render()` renders a centered 50x5 dialog (clamped to terminal size) with:
- Yellow borders, title `" Confirm "`
- Message: `"{action_label} {unit_name}?"` (or just `"{action_label}?"` for actions that do not need a unit)
- Prompt: `"[y]es / any key to cancel"`

### Context Menu

`context_menu::render_context_menu()` renders a `List` widget at the mouse click position (`menu.x`, `menu.y`). The menu is clamped to stay within `frame_size` via `compute_menu_rect()`. Items are displayed with yellow borders and bold yellow-on-black highlight for the selected item.

Two context menu targets exist:
- **SidebarService**: Shows service actions (start, stop, restart, etc.)
- **Pane**: Shows pane management actions (split horizontal/vertical, split with new service, close)

### Split Prompt

`render_split_prompt()` renders a small centered 40x3 dialog with yellow borders, title `" Split "`, and the text `"[h]orizontal / [v]ertical"`.

### Status Bar

Rendered inline in `render()` as a single-line `Paragraph` with spans showing:
- `[Scope: {filter_mode}]` - User/System/Both
- `[Status: {status_filter}]` - All/Active/Inactive/Failed
- `[Mode: {list_mode} (count)]` - All/Include (N)/Exclude (N)
- `[Sort: {sort_mode}]` - Name/Status/Uptime

Labels are DarkGray, values are Cyan.

---

## Related Files

| File | Role | Key Exports/Interfaces |
|------|------|----------------------|
| `src/ui/mod.rs` | Top-level render orchestration and layout computation | `render()`, `LayoutCache`, `render_split_prompt()` |
| `src/ui/sidebar.rs` | Service list rendering with status icons and selection | `render_sidebar()` |
| `src/ui/detail.rs` | Selected service detail panel | `render_detail()` |
| `src/ui/logs.rs` | Log pane rendering with scrollbar and search highlighting | `render_log_pane()` |
| `src/ui/search.rs` | Search bar input widget | `SearchBar` (implements `Widget`) |
| `src/ui/help.rs` | Help bar and help overlay rendering | `render_help_bar()`, `render_help_overlay()` |
| `src/ui/confirm.rs` | Confirmation dialog widget | `ConfirmDialog` (struct with `render()` method) |
| `src/ui/context_menu.rs` | Context menu rendering and rect computation | `render_context_menu()`, `compute_menu_rect()`, `ContextMenu`, `ContextMenuItem`, `ContextMenuAction`, `ContextMenuTarget` |
| `src/ui/panes.rs` | Pane tree data structure and layout computation | `PaneTree`, `PaneLeaf`, `PaneNode`, `PaneSplit`, `PaneId`, `SplitDirection` |
| `src/app.rs` | Application state consumed by rendering; stores `LayoutCache`; defines `InputMode`, `HitTarget`, `FilterMode`, `StatusFilter`, `ListMode`, `SortMode` | `App`, `InputMode`, `HitTarget`, `LayoutCache` (stored on `App`) |
| `src/main.rs` | Calls `ui::render()` in the draw loop and stores the returned `LayoutCache` on `App` | Main event loop (lines 106-113) |
| `src/config/keys.rs` | Keybinding definitions consumed by help bar and help overlay | `KeyBindings`, `KeyAction`, `format_key_event()` |
| `src/journal/filter.rs` | Search match computation used by log pane highlighting | `find_matches()` |
| `src/systemd/types.rs` | `UnitInfo`, `ServiceDetail`, `ActiveState` types read during rendering | `UnitInfo`, `ServiceDetail`, `ActiveState` |

---

## Invariants and Constraints

1. **Rendering is stateless**: `render()` takes an immutable `&App` reference, reads state, writes to the `Frame`, and returns a `LayoutCache`. It never mutates application state. The only mutable parameter is `&mut ListState` (ratatui's internal scroll tracking for the sidebar list widget).

2. **LayoutCache must be recomputed every frame**: The cache is rebuilt from scratch on every render call. On terminal resize, the ratatui `Frame` automatically receives the new terminal size, so layout adapts without explicit resize handling. The cached rectangles are only valid until the next render.

3. **Overlays render last**: All overlay widgets (`ConfirmDialog`, help overlay, split prompt, context menu) are rendered after all base content. They use `ratatui::widgets::Clear` to erase the background behind them before drawing, ensuring they appear on top.

4. **Sidebar width constraints**: Sidebar width is `min(35, content_width / 3)`. This ensures the sidebar never exceeds 35 characters and never takes more than one-third of the available width, leaving adequate space for the main panel.

5. **Detail panel has fixed height**: The detail panel is always exactly 9 rows (`Constraint::Length(9)`). This provides consistent space for the service metadata fields regardless of terminal height. The remaining vertical space goes to log panes.

6. **Log pane border color indicates focus**: Focused panes have Cyan borders, unfocused panes have DarkGray borders. Only one pane can be focused at a time (`app.focused_pane`). The sidebar follows the same convention (Cyan when `InputMode::Normal`, DarkGray otherwise).

7. **Detail panel borders are always DarkGray**: The detail panel is not a focusable element and always uses DarkGray borders.

8. **Search bar occupies space, not an overlay**: Unlike the help screen and confirmation dialog which render on top of existing content, the search bar is inserted into the layout flow and pushes content down by 3 rows. It only appears when `InputMode` is `SearchServices` or `SearchLogs`.

9. **Log buffer cap**: Each `PaneLeaf` maintains a `VecDeque<String>` capped at 10,000 lines (`MAX_LOG_LINES`). When the cap is exceeded, the oldest line is removed and `scroll_offset` is decremented to keep the view stable.

10. **Scroll offset semantics**: `scroll_offset == 0` means "following" (tail mode, showing latest lines). A positive offset means the view is scrolled back by that many lines from the bottom. The LIVE indicator and +N counter reflect this directly.

11. **Context menu position clamping**: `compute_menu_rect()` ensures the context menu rectangle stays within the terminal bounds by shifting it left or up when it would extend past the right or bottom edge.

12. **Overlay centering**: All overlay dialogs (confirm, help, split prompt) are centered within the full terminal area. Their dimensions are clamped to not exceed `terminal_size - margin` (typically 4 for help, 2-4 for dialogs).
