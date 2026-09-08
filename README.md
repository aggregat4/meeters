This is a graphical (GTK4) utility for Linux that lives in the tray as an app indicator, watches a configured ical calendar file URL and will notify shortly before a meeting begins. It allows you to directly open any (Zoom) embedded meeting URL with a single click from either the popup menu or the notification.

# Building

Requires Rust 1.92 or newer, GTK4 development libraries, and D-Bus development libraries.
On Ubuntu/Debian, install them with `sudo apt install libgtk-4-dev libdbus-1-dev`.

1. Clone repo
1. `cargo build --locked`

# Installation

You can drop the meeters binary anywhere. The desktop must provide a StatusNotifierItem tray host (for example, KDE Plasma’s tray or GNOME Shell with an AppIndicator/StatusNotifierItem extension). GTK3 and libappindicator are no longer required.

The tarball includes a few (optional) icons that will be used when they are located next to the meeters binary. If not the program will default to a "new appointment" icon.

# Configuration

meeters can be configured using environment variables, or a configuration file or a mix of both.

When using a file meeters expects a configuration file called `meeters_config.env` in a directory called `meeters` in your Linux standard config location. This will typically be: `~/.config/meeters/meeters_config.env`

The file should have name/value pairs separated by equals signs. For example:

```
MEETERS_ICAL_URL=http://example.com/calendar.ics
```

For an on-prem Exchange calendar via EWS:

```
MEETERS_CALENDAR_SOURCE=ews
MEETERS_EWS_URL=https://mail.example.com/EWS/Exchange.asmx
MEETERS_EWS_USER=user@example.com
```

When using EWS, meeters asks for the Exchange password in the UI on first refresh and stores it
in the desktop keyring/wallet. EWS requests use Basic auth over HTTPS.

ICS and EWS use reusable Ureq HTTP clients. EWS requests do not follow redirects.
The pinned Ureq version reads proxy environment variables but does not honor
`NO_PROXY` exclusions. If an internal Exchange server must be reached directly,
launch meeters with the proxy environment variables unset.

The EWS password is stored through the FreeDesktop Secret Service API, which is typically backed
by GNOME Keyring, KDE Wallet, or a compatible provider such as KeePassXC. The keyring service name
is:

```
net.aggregat4.meeters.exchange
```

The entry user is the configured `MEETERS_EWS_USER`. On KDE, inspect or delete the entry with
KWalletManager. If `secret-tool` is installed, the stored password can also be checked with:

```bash
secret-tool lookup service net.aggregat4.meeters.exchange username user@example.com
```

The following properties are supported:

| Property | Required | Default Value | Description |
|----------|----------|---------------|-------------|
| MEETERS_CALENDAR_SOURCE | no | ics | Calendar source to use. Supported values: `ics` and `ews`. |
| MEETERS_ICAL_URL | yes for `ics` | - | The HTTP URL to your ical calendar |
| MEETERS_ICAL_USER_AGENT | no | Firefox 154 for Linux | The HTTP `User-Agent` sent when retrieving an ICS calendar. Defaults to `Mozilla/5.0 (X11; Linux x86_64; rv:154.0) Gecko/20100101 Firefox/154.0`. Some (all?) versions of Exchange no longer accept requests with non-browser User-Agent headers. |
| MEETERS_EWS_URL | yes for `ews` | - | The direct Exchange Web Services endpoint, typically ending in `/EWS/Exchange.asmx`. |
| MEETERS_EWS_USER | yes for `ews` | - | The Exchange user in email form, for example `user@example.com`. |
| MEETERS_LOCAL_TIMEZONE | no | Europe/Berlin | The local timezone where all times will be converted to. Make sure you set this to a valid IANA timezone identifier if you are not in the default timezone |
| MEETERS_EVENT_NOTIFICATION | no | true | Whether or not an upcoming event should be announced with a sticky notification ("true" or "false") | 
| MEETERS_POLLING_INTERVAL_MS | no | 120000 | The time in milliseconds between two fetches of the ical calendar. |
| MEETERS_EVENT_WARNING_TIME_SECONDS | no | 60 | The time in seconds before the next meeting to show the notification. |
| MEETERS_FUTURE_DAYS | no | 1 | The number of future days to show in the calendar view in addition to today. For example, a value of 1 shows today and tomorrow, 2 shows today plus two more days, etc. |
| MEETERS_TODAY_START_HOUR | no | 8 | The start hour of the timeline view (0-23). Events before this hour will not be visible in the timeline. |
| MEETERS_TODAY_END_HOUR | no | 20 | The end hour of the timeline view (0-23). Events after this hour will not be visible in the timeline. |
| MEETERS_USE_ZOOMMTG | no | false | If set to true, Zoom meeting URLs will be opened with `zoommtg://` instead of `https://`. |
| MEETERS_LOG | no | warn | Controls stderr log verbosity. Supported values: `off`, `error`, `warn`, `info`, `debug`/`verbose`, and `trace`. |


# D-Bus Interface

meeters exposes a D-Bus interface that allows you to control the window state programmatically. The service name is `net.aggregat4.Meeters` and the object path is `/net/aggregat4/Meeters`.

## Available D-Bus Commands

### ShowWindow
Opens the meetings window if it's closed or creates it if it doesn't exist.

```bash
dbus-send --session --dest=net.aggregat4.Meeters --type=method_call /net/aggregat4/Meeters net.aggregat4.Meeters.ShowWindow
```

### CloseWindow
Hides the meetings window if it's open.

```bash
dbus-send --session --dest=net.aggregat4.Meeters --type=method_call /net/aggregat4/Meeters net.aggregat4.Meeters.CloseWindow
```

### ToggleWindow
Toggles the window state - opens it if it's closed or closes it if it's open.

```bash
dbus-send --session --dest=net.aggregat4.Meeters --type=method_call /net/aggregat4/Meeters net.aggregat4.Meeters.ToggleWindow
```

# Troubleshooting

| Error | Solution |
|-------|----------|
| Tray icon does not appear | Enable a StatusNotifierItem-compatible tray host or extension. Meeters waits for the tray host to become available. Launching Meeters again shows the existing calendar window; Ctrl+Q quits the application. |

# Desktop migration tests

The GTK4 migration keeps the existing D-Bus control interface and uses a separate
`net.aggregat4.Meeters.Application` ID for single-instance application activation.
Closing the calendar hides it; the application continues polling in the tray.

The isolated smoke tests require `xvfb`, `xauth`, `xdotool`, `dbus-x11`,
`python3-dbus`, and `python3-gi` on Ubuntu/Debian:

```bash
cargo build --locked
cargo test --locked
dbus-run-session -- xvfb-run -a python3 tests/desktop_smoke.py target/debug/meeters
GTK_A11Y=none GSK_RENDERER=cairo dbus-run-session -- xvfb-run -a cargo test --locked gtk4_window_and_password_dialogs -- --ignored --test-threads=1
```

The smoke test provides its own calendar and tray host. It does not use your
calendar configuration or keyring. Before releasing, also check tray icon rendering,
notification links, password storage, and window presentation on the target desktop,
including Wayland where supported.

# Linux compatibility baseline

Native release binaries target **Ubuntu 22.04 LTS and newer**, using Ubuntu 22.04's
system GTK 4.6 and glibc 2.35 as the compatibility baseline. New Rust bindings do
not authorize enabling APIs that require newer system libraries.

CI builds and runs the release binary on Ubuntu 22.04 with both Rust 1.92.0 and
stable, including the desktop smoke test. CI and release packaging also reject
binaries requiring glibc symbols newer than 2.35:

```bash
python3 tests/check_glibc.py target/release/meeters --max-version 2.35
```

Build distributable binaries on Ubuntu 22.04 (or in a matching container), not on
a newer developer workstation. The glibc check supplements the baseline runtime
tests; it does not by itself verify GTK or other shared-library compatibility.
