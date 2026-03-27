# D-Bus Communication

## Scope

### In Scope

- Establishing connections to both the system and session D-Bus buses via `zbus`
- Listing all loaded systemd units by calling the `org.freedesktop.systemd1.Manager.ListUnits` method
- Subscribing to systemd Manager signals (`Subscribe`) so that `UnitNew` and `UnitRemoved` signals are delivered
- Listening for `PropertiesChanged`, `UnitNew`, and `UnitRemoved` D-Bus signals in background tasks
- Forwarding D-Bus signals as typed `AppEvent` variants through a `tokio::sync::mpsc` channel to the main event loop
- Querying detailed unit/service properties via `org.freedesktop.DBus.Properties.GetAll` for both the `Unit` and `Service` interfaces
- Keeping the `App.all_units` list and `App.unit_details` cache in sync with real-time D-Bus signals
- Mapping raw D-Bus tuple responses into strongly-typed Rust structs (`UnitInfo`, `ServiceDetail`)
- Tracking `BusType` (System vs Session) per unit to route queries to the correct bus connection

### Not In Scope

- Command execution via `systemctl` (covered by `src/systemd/commands.rs`, which shells out to the `systemctl` binary rather than using D-Bus methods)
- UI rendering of unit lists and details
- Journal/log streaming (separate subsystem)
- Filtering, sorting, and search logic (these consume the data this feature provides but are not part of the D-Bus layer)

---

## Data/Control Flow

### 1. Connection Establishment

At startup (`main.rs:41-42`), two D-Bus connections are opened:

```
main()
  -> dbus::system_bus()   -> Connection::system().await   -> system D-Bus connection
  -> dbus::session_bus()  -> Connection::session().await   -> session D-Bus connection
```

Both functions wrap the `zbus::Connection` constructors with `anyhow::Context` for error reporting. Both connections must succeed for the application to start; failure is a fatal error propagated via `?`.

### 2. Signal Subscription

Before spawning signal listeners, the application subscribes to systemd Manager signals on each bus (`main.rs:45-50`):

```
main()
  -> dbus::subscribe(&system_bus)   -- calls org.freedesktop.systemd1.Manager.Subscribe
  -> dbus::subscribe(&session_bus)  -- calls org.freedesktop.systemd1.Manager.Subscribe
```

`Subscribe()` tells the systemd Manager to emit `UnitNew` and `UnitRemoved` signals on the bus. Without this call, systemd does not broadcast lifecycle signals to the connection. Subscription failures are logged as warnings but are not fatal -- the application can still function with manual refreshes but will miss real-time updates.

### 3. Initial Unit Loading

During `App::new()` (`app.rs:247`), units are fetched for the first time:

```
App::new()
  -> app.load_units()
       -> dbus::list_units(&system_bus, BusType::System)  -- if filter includes system
       -> dbus::list_units(&session_bus, BusType::Session) -- if filter includes session
       -> retain only services (is_service())
       -> store in self.all_units
  -> app.apply_filters()
       -> produce self.filtered_units from self.all_units
```

#### list_units() internals

1. Builds a `zbus::Proxy` targeting `org.freedesktop.systemd1` at path `/org/freedesktop/systemd1` on interface `org.freedesktop.systemd1.Manager`
2. Calls `ListUnits` with no arguments
3. Deserializes the response as `Vec<(String, String, String, String, String, String, OwnedObjectPath, u32, String, OwnedObjectPath)>` -- the 10-element tuple that systemd returns per unit
4. Maps each tuple into a `UnitInfo` struct, deriving `UnitKind` from the unit name suffix and parsing `LoadState`/`ActiveState` from their string representations
5. Attaches the provided `BusType` to each `UnitInfo` so queries can later be routed to the correct bus

### 4. Signal Listener Spawning

Two signal listeners are spawned as background tokio tasks (`main.rs:61-66`):

```
main()
  -> dbus::spawn_signal_listener(system_bus.clone(), BusType::System, tx.clone())
  -> dbus::spawn_signal_listener(session_bus.clone(), BusType::Session, tx.clone())
```

Each call to `spawn_signal_listener` creates a top-level `tokio::spawn` that itself spawns two sub-tasks:

#### Sub-task A: PropertiesChanged listener

- Match rule: `type=signal, interface=org.freedesktop.DBus.Properties, member=PropertiesChanged`
- Creates a `zbus::MessageStream` for the match rule
- For each signal received:
  1. Extracts the object path from the message header
  2. Deserializes the body as `(String, HashMap<String, OwnedValue>, Vec<String>)` -- the standard `PropertiesChanged` signature (interface name, changed properties, invalidated properties)
  3. Sends `AppEvent::PropertiesChanged { path, bus_type, changed }` through the mpsc channel

#### Sub-task B: UnitNew/UnitRemoved listener

- Match rule: `type=signal, path=/org/freedesktop/systemd1, interface=org.freedesktop.systemd1.Manager`
- Creates a `zbus::MessageStream` for the match rule
- For each signal received:
  1. Checks the member name from the message header
  2. For `UnitNew`: deserializes `(String, OwnedObjectPath)` and sends `AppEvent::UnitNew { name, path, bus_type }`
  3. For `UnitRemoved`: deserializes `(String, OwnedObjectPath)` and sends `AppEvent::UnitRemoved { name, path, bus_type }`
  4. Other signals on this interface are ignored

### 5. Event Handling in App

The main event loop (`main.rs:76-115`) calls `app.handle_event(event)` for each `AppEvent`. The D-Bus-related branches are:

#### AppEvent::UnitNew

`handle_unit_new(name, bus_type)` (`app.rs:1207-1221`):
1. Selects the correct bus connection based on `bus_type`
2. Re-fetches the full unit list for that bus via `dbus::list_units()`
3. Removes all existing units of that `bus_type` from `all_units`
4. Extends `all_units` with the fresh list (filtered to services only)
5. Calls `apply_filters()` to rebuild `filtered_units`

This is a full re-list rather than a single-unit insert because the `UnitNew` signal does not carry enough information to construct a complete `UnitInfo`.

#### AppEvent::UnitRemoved

`handle_unit_removed(name)` (`app.rs:1223-1228`):
1. Removes the unit from `all_units` by name
2. Removes any cached `ServiceDetail` from `unit_details`
3. Calls `apply_filters()` to rebuild `filtered_units`

#### AppEvent::PropertiesChanged

`handle_properties_changed(path, bus_type, changed)` (`app.rs:1230-1264`):
1. Finds the unit in `all_units` whose `object_path` matches the signal's path
2. If found, selects the correct bus connection based on `bus_type`
3. Calls `dbus::get_service_detail(conn, path)` to re-fetch the full property set
4. On success:
   - Updates the unit's `active_state` and `sub_state` in `all_units`
   - Inserts/updates the `ServiceDetail` in `unit_details`
   - Calls `apply_filters()` to reflect any state changes in the filtered view
5. On failure, logs a debug message (the unit may have been removed between signal delivery and property fetch)

### 6. Detail Fetching on Tick

`handle_tick()` (`app.rs:1266-1283`) runs at ~4 Hz and lazily fetches details for the currently selected unit:
1. If a unit is selected and its details are not in `unit_details`, fetches them via `get_service_detail()`
2. Routes the query to the correct bus based on the unit's `bus_type`
3. Caches the result in `unit_details`

#### get_service_detail() internals

1. Builds a proxy targeting `org.freedesktop.DBus.Properties` on the unit's object path
2. Calls `GetAll("org.freedesktop.systemd1.Unit")` to fetch unit-level properties: `ActiveState`, `SubState`, `FragmentPath`, `UnitFileState`, `Description`, `Requires`, `Wants`, `After`
3. Calls `GetAll("org.freedesktop.systemd1.Service")` to fetch service-level properties: `MainPID`, `MemoryCurrent`, `ExecMainStartTimestamp`
4. Both calls are fault-tolerant: if either `GetAll` fails (e.g., the unit is not a service), the corresponding fields keep their default values
5. Property values are extracted via helper functions (`try_string`, `try_string_vec`, `try_u32`, `try_u64`) that use `zbus::zvariant` downcasting and return sensible defaults on type mismatch

---

## Related Files

| File | Role | Key Exports/Interfaces |
|------|------|----------------------|
| `src/systemd/mod.rs` | Module declaration | Re-exports `commands`, `dbus`, `types` submodules |
| `src/systemd/dbus.rs` | D-Bus connection, queries, and signal listening | `system_bus()`, `session_bus()`, `list_units()`, `subscribe()`, `get_service_detail()`, `spawn_signal_listener()` |
| `src/systemd/types.rs` | Type definitions for systemd domain objects | `BusType`, `ActiveState`, `LoadState`, `UnitFileState`, `UnitKind`, `UnitInfo`, `ServiceDetail` |
| `src/systemd/commands.rs` | systemctl command execution (out of scope but related) | `ServiceAction`, `execute_systemctl()`, `edit_unit_file()` |
| `src/event.rs` | Event type definitions and event handler infrastructure | `AppEvent` (variants: `UnitNew`, `UnitRemoved`, `PropertiesChanged`), `EventHandler` |
| `src/main.rs` | Application bootstrap: bus connection, subscription, listener spawning, main event loop | N/A (orchestration only) |
| `src/app.rs` | Application state and event handling | `App` (fields: `all_units`, `unit_details`, `system_bus`, `session_bus`), `App::load_units()`, `App::handle_unit_new()`, `App::handle_unit_removed()`, `App::handle_properties_changed()`, `App::handle_tick()` |

### Type Definitions

- **`BusType`** (`types.rs`): `System | Session` -- identifies which D-Bus bus a unit belongs to. Implements `Display` (System -> "system", Session -> "user").
- **`ActiveState`** (`types.rs`): `Active | Inactive | Failed | Activating | Deactivating | Maintenance | Reloading | Unknown` -- parsed from systemd's string representation.
- **`LoadState`** (`types.rs`): `Loaded | NotFound | BadSetting | Error | Masked | Unknown`.
- **`UnitFileState`** (`types.rs`): `Enabled | Disabled | Static | Masked | Indirect | Generated | Transient | BadSetting | Unknown`.
- **`UnitKind`** (`types.rs`): `Service | Timer | Socket | Mount | Target | Path | Scope | Slice | Device | Automount | Swap | Snapshot | Unknown` -- derived from the unit name's file extension.
- **`UnitInfo`** (`types.rs`): The primary unit record with fields: `name`, `description`, `load_state`, `active_state`, `sub_state`, `unit_kind`, `bus_type`, `object_path`. Methods: `short_name()`, `is_service()`.
- **`ServiceDetail`** (`types.rs`): Detailed properties for a single unit with fields: `active_state`, `sub_state`, `main_pid`, `memory_current`, `exec_main_start_timestamp`, `fragment_path`, `unit_file_state`, `requires`, `wants`, `after`, `description`. Methods: `memory_human()`, `uptime_human()`.
- **`AppEvent`** (`event.rs`): The unified event enum. D-Bus-relevant variants:
  - `UnitNew { name: String, path: String, bus_type: BusType }`
  - `UnitRemoved { name: String, path: String, bus_type: BusType }`
  - `PropertiesChanged { path: String, bus_type: BusType, changed: HashMap<String, OwnedValue> }`

---

## Invariants and Constraints

1. **Both buses must connect at startup.** `system_bus()` and `session_bus()` are called with `?` in `main()`. If either connection fails, the application exits immediately. There is no fallback to a single-bus mode.

2. **Signal subscription failures are non-fatal.** If `subscribe()` fails on either bus, the application continues but will not receive `UnitNew`/`UnitRemoved` signals for that bus. This is logged as a warning.

3. **Signal listeners run as background tokio tasks.** They are fire-and-forget (`tokio::spawn` with no join handle retained). If a listener task panics or the stream ends, there is no restart mechanism -- D-Bus updates for that bus will silently stop.

4. **The unit list may change at any time via signals.** `all_units` is not behind a lock because the `App` is single-threaded (owned by the main event loop). All mutations happen inside `handle_event()`, which processes events sequentially.

5. **D-Bus property queries can fail.** A unit may be removed between the time a signal is received and the time `get_service_detail()` executes. All property fetch failures are handled gracefully: `handle_properties_changed` logs a debug message, `handle_tick` silently skips, and `get_service_detail` itself uses `Ok`-returning fallbacks for individual `GetAll` calls.

6. **`BusType` must be tracked per unit to route queries to the correct bus.** Each `UnitInfo` carries its `bus_type`. When the application needs to query properties or re-list units, it matches on `bus_type` to select `self.system_bus` or `self.session_bus`. Using the wrong bus would result in "unit not found" errors.

7. **`UnitNew` triggers a full re-list for the affected bus.** Rather than inserting a single unit (which would require another D-Bus call to get its full info), the handler removes all units of the same `bus_type` from `all_units` and replaces them with a fresh `list_units()` result. This ensures consistency but is an O(n) operation on each `UnitNew` signal.

8. **`UnitRemoved` performs a targeted removal.** The unit is removed from `all_units` by name, and its cached `ServiceDetail` is also removed from `unit_details`.

9. **`PropertiesChanged` updates are path-matched.** The signal carries the D-Bus object path, not the unit name. The handler must search `all_units` for a matching `object_path` to determine which unit was affected. If no match is found (e.g., the signal is for a non-service unit type), the event is silently ignored.

10. **Only service units are tracked.** `load_units()` calls `retain(|u| u.is_service())` after listing all units. Non-service unit types (timers, sockets, mounts, etc.) are discarded even though `UnitKind` can represent them. The type system supports all unit kinds for potential future use.

11. **Detail fetching is lazy.** `ServiceDetail` is only fetched when a unit is selected (via `handle_tick`) or when a `PropertiesChanged` signal arrives for a unit already in `all_units`. There is no bulk pre-fetch of details for all units.

12. **The mpsc channel is unbounded.** `AppEvent` uses `tokio::sync::mpsc::unbounded_channel`, meaning D-Bus signal bursts will not apply backpressure to the signal listeners. Under extreme signal load, memory usage could grow until the main loop processes the queue.
