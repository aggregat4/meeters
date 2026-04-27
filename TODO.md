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

- [x] Replace the 100ms `glib::timeout_add_local` polling loops for calendar messages and D-Bus messages in `src/main.rs`.
- [x] Investigate `glib::MainContext::channel`, `invoke`, or another GTK-main-thread-safe dispatch mechanism compatible with the current GTK/glib versions. Use `async_channel::Receiver::recv().await` inside `glib::MainContext::spawn_local`, because this glib version deprecates `MainContext::channel` in favor of async-channel on the main context.
- [x] Keep all GTK mutation on the GTK main thread.
- [x] Add graceful handling for closed channels instead of assuming `send_blocking(...).unwrap()` always succeeds.

## 6. Improve runtime logging

- [x] Replace ad hoc `println!` and `eprintln!` calls with `log` or `tracing`.
- [x] Add log levels for normal refresh status, parse failures, notification failures, D-Bus startup, and icon state transitions.
- [x] Keep default output quiet enough for normal desktop autostart use.
- [x] Consider exposing verbose logging through an env var.

## 7. Split `src/gui.rs` into focused modules

- [x] Split tray/appindicator code, timeline rendering, window management, refresh log dialog, D-Bus setup, and notifications into separate modules.
- [x] Keep public APIs small: `initialize_gui`, `run_gui_main_loop`, and a small window/tray coordination surface.
- [x] Do this after extracting duplicated day view code, so the split is mostly moving already-clean functions.

## 8. Clean up naming and review comments

- [x] Rename `get_icon_path_with_fallbak` to `get_icon_path_with_fallback`.
- [x] Keep `read_fucked_windows_zones` and related identifiers unchanged; the naming intentionally documents the cost of dealing with Outlook/Windows timezone data.
- [x] Produce a concrete list of comments proposed for removal or rewrite before editing them.
- [x] Only remove comments after review; preserve comments that explain non-obvious calendar/timezone behavior, GTK constraints, or compatibility workarounds.

## 9. Expand behavior-focused tests

- [ ] Add parser tests around malformed but realistic iCal input.
- [ ] Add interval filtering tests for all-day events, events crossing midnight, events starting exactly at interval boundaries, and events ending exactly at interval boundaries.
- [ ] Add config validation tests once `Config` exists.
- [ ] Add tests for notification de-duplication logic if it gets extracted from the background loop.
- [ ] Add tests for adjacent event layout math if timeline positioning is extracted into pure helper functions.

## 10. Revisit tray integration deprecation

- [ ] Track whether Rust bindings for `libayatana-appindicator-glib` become practical.
- [ ] Evaluate whether a StatusNotifierItem implementation would avoid the deprecated `libayatana-appindicator` stack.
- [ ] Update README installation notes if/when the tray dependency changes.

## Older Notes

- [x] Add an icon state for no events.
- [x] Reflect iCal fetching error in error state in icon.
- [ ] Investigate adding secrets support for basic authenticated URLs.
- [x] Add optional support for opening Zoom meetings directly through `zoommtg://`.
