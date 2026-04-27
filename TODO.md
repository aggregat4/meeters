# TODO

## 1. Harden calendar parsing against malformed events

- [x] Replace runtime `unwrap()` calls in `src/meeters_ical.rs` with `CalendarError` returns where malformed iCal input can trigger them.
- [x] Start with high-risk properties: `DTSTART`, `DTEND`, `UID`, `RECURRENCE-ID`, property `value`, timezone conversion results, and RRULE `UNTIL` conversion.
- [x] Do not silently ignore malformed events. Prefer preserving the current all-or-nothing refresh failure first, with a clear `CalendarError` shown in the refresh log.
- [x] Add tests for missing `DTSTART`, missing `UID` on recurring events, malformed timezone IDs, malformed `RRULE`, and invalid recurrence modifications.

## 2. Centralize configuration parsing and validation

- [x] Introduce a `Config` struct, likely in `src/main.rs` first or a new `src/config.rs` module if it grows.
- [x] Move all `dotenvy::var(...)` parsing into `Config::load() -> Result<Config, ...>`.
- [x] Validate value ranges instead of only parse types: `MEETERS_TODAY_START_HOUR` and `MEETERS_TODAY_END_HOUR` must be `0..=23`, `start_hour < end_hour`, `MEETERS_FUTURE_DAYS >= 0`, polling interval > 0, warning time >= 0.
- [x] Return actionable errors instead of panicking for invalid config.
- [x] Add focused unit tests for defaults, valid overrides, invalid booleans, invalid hour ranges, and invalid future day counts.

## 3. Extract duplicated day view construction

- [x] Remove duplicated UI construction between `WindowManager::show_window` and `WindowManager::update_events` in `src/gui.rs`.
- [x] Extract helpers such as `build_days_view(...)`, `day_label_text(...)`, and possibly `build_day_box(...)`.
- [x] Keep behavior unchanged: same day labels, same timeline creation, same scrolled window policy.
- [x] After extraction, future styling and layout changes should happen in one place only.

## 4. Improve timeline sizing and layout responsiveness

- [x] Replace raw timeline/window width literals (`600`, `700 * days`) with named constants in `src/gui.rs`.
- [x] Investigate GTK allocation-based sizing while preserving exact vertical positioning. The attempted `gtk::Fixed` allocation callback plus `set_size_request` approach caused resize locking and event-height regressions, so responsive width needs a different layout strategy.

## 5. Replace GUI-thread channel polling with GLib-friendly event delivery

- [ ] Replace the 100ms `glib::timeout_add_local` polling loops for calendar messages and D-Bus messages in `src/main.rs`.
- [ ] Investigate `glib::MainContext::channel`, `invoke`, or another GTK-main-thread-safe dispatch mechanism compatible with the current GTK/glib versions.
- [ ] Keep all GTK mutation on the GTK main thread.
- [ ] Add graceful handling for closed channels instead of assuming `send_blocking(...).unwrap()` always succeeds.

## 6. Improve runtime logging

- [ ] Replace ad hoc `println!` and `eprintln!` calls with `log` or `tracing`.
- [ ] Add log levels for normal refresh status, parse failures, notification failures, D-Bus startup, and icon state transitions.
- [ ] Keep default output quiet enough for normal desktop autostart use.
- [ ] Consider exposing verbose logging through an env var.

## 7. Split `src/gui.rs` into focused modules

- [ ] Split tray/appindicator code, timeline rendering, window management, refresh log dialog, D-Bus setup, and notifications into separate modules.
- [ ] Keep public APIs small: `initialize_gui`, `run_gui_main_loop`, and a small window/tray coordination surface.
- [ ] Do this after extracting duplicated day view code, so the split is mostly moving already-clean functions.

## 8. Introduce a richer error type

- [ ] Replace `CalendarError { msg: String }` with a typed error enum, probably using `thiserror`.
- [ ] Preserve user-readable messages for the tray refresh log.
- [ ] Include categories for config, fetch, iCal parse, timezone parse, recurrence parse, and GUI integration errors.
- [ ] Retain source errors where useful so debugging does not lose context.

## 9. Clean up naming and comments

- [ ] Rename `get_icon_path_with_fallbak` to `get_icon_path_with_fallback`.
- [ ] Rename `read_fucked_windows_zones` and related identifiers to neutral names such as `read_nonstandard_windows_zones`.
- [ ] Remove or rewrite stale comments that describe historical experiments rather than current behavior.
- [ ] Keep comments only where they explain non-obvious calendar/timezone behavior or GTK constraints.

## 10. Expand behavior-focused tests

- [ ] Add parser tests around malformed but realistic iCal input.
- [ ] Add interval filtering tests for all-day events, events crossing midnight, events starting exactly at interval boundaries, and events ending exactly at interval boundaries.
- [ ] Add config validation tests once `Config` exists.
- [ ] Add tests for notification de-duplication logic if it gets extracted from the background loop.
- [ ] Add tests for adjacent event layout math if timeline positioning is extracted into pure helper functions.

## 11. Revisit tray integration deprecation

- [ ] Track whether Rust bindings for `libayatana-appindicator-glib` become practical.
- [ ] Evaluate whether a StatusNotifierItem implementation would avoid the deprecated `libayatana-appindicator` stack.
- [ ] Update README installation notes if/when the tray dependency changes.

## Older Notes

- [x] Add an icon state for no events.
- [x] Reflect iCal fetching error in error state in icon.
- [ ] Investigate adding secrets support for basic authenticated URLs.
- [x] Add optional support for opening Zoom meetings directly through `zoommtg://`.
