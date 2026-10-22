# Changelog

## 1.13.0 (pending release)

### Features

- Migrate the desktop interface to GTK4 and a StatusNotifierItem tray icon.
  Launching another instance presents the existing calendar window; closing the
  window keeps the application running in the tray.
- Export EWS calendar summaries as JSON with
  `meeters --calendar-until YYYY-MM-DD`. The command includes the requested local
  date and uses the configured calendar and saved keyring password without
  starting GTK. It rejects truncated server results instead of emitting an
  incomplete calendar.
- Show calendar location text when no EWS room resources are available, including
  locations from ICS calendars. Keep room response statuses and declined-room
  warnings when resource information is available.
- Improve meeting details with selectable text, scrollable descriptions, and a
  dedicated join button. Use compact layouts for short meetings and reuse timeline
  columns when meetings no longer overlap.

### Fixes

- Correct signed timezone offsets and Exchange daylight-saving transition
  instants, including fractional negative offsets.
- Handle ambiguous and nonexistent local times, historical timezone observances,
  and recurrence exclusions expressed in different timezones.
- Correct Exchange all-day recurrence exceptions.
- Fix GTK4 event popover handling and avoid displaying a standalone meeting URL
  as a redundant physical location.

### Dependencies and validation

- Replace the recurrence fork with upstream `rrule` and update timezone data and
  other dependencies, including updates addressing dependency security advisories.
- Reuse Ureq HTTP clients for ICS and EWS and remove unused dependencies.
- Add deterministic calendar regression fixtures, checks across host timezones,
  desktop smoke tests, and GTK dialog tests.
- Test builds with Rust 1.92.0 and stable on Ubuntu 22.04 and reject release
  binaries requiring glibc newer than 2.35.
- Reconcile the Cargo package version with the GitHub release version.
- Build release archives when a release is published, including releases prepared
  as drafts.

### Installation and compatibility

- Linux binaries target **Ubuntu 22.04 LTS or newer**, with GTK 4.6 and glibc 2.35
  as the baseline. The `linux-x86` archive contains an **x86_64** binary.
- A StatusNotifierItem-compatible tray host is required, such as KDE Plasma's tray
  or GNOME Shell with an AppIndicator/StatusNotifierItem extension. GTK3 and
  libappindicator are no longer required.
- Building from source requires Rust 1.92 or newer, GTK4 development libraries,
  and D-Bus development libraries.
- Existing configuration and D-Bus window controls remain supported. JSON export
  requires EWS and an existing saved password; ICS export is not supported.

[Changes since v1.12.0](https://github.com/aggregat4/meeters/compare/v1.12.0...main)
