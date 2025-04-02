This is a graphical (GTK) utility for Linux that lives in the tray as an app indicator, watches a configured ical calendar file URL and will notify shortly before a meeting begins. It allows you to directly open any (Zoom) embedded meeting URL with a single click from either the popup menu or the notification.

# Building

1. Clone repo
1. `cargo b`

# Installation

You can drop the meeters binary anywhere. The tarball includes a few (optional) icons that will be used when they are located next to the meeters binary. If not the program will default to a "new appointment" icon.

# Configuration

meeters can be configured using environment variables, or a configuration file or a mix of both.

When using a file meeters expects a configuration file called `meeters_config.env` in a directory called `meeters` in your Linux standard config location. This will typically be: `~/.config/meeters/meeters_config.env`

The file should have name/value pairs separated by equals signs. For example:

```
MEETERS_ICAL_URL=http://example.com/calendar.ics
```

The following properties are supported:

| Property | Required | Default Value | Description |
|----------|----------|---------------|-------------|
| MEETERS_ICAL_URL | yes | - | The HTTP URL to your ical calendar |
| MEETERS_LOCAL_TIMEZONE | no | Europe/Berlin | The local timezone where all times will be converted to. Make sure you set this to a valid IANA timezone identifier if you are not in the default timezone |
| MEETERS_EVENT_NOTIFICATION | no | true | Whether or not an upcoming event should be announced with a sticky notification ("true" or "false") | 
| MEETERS_POLLING_INTERVAL_MS | no | 120000 | The time in milliseconds between two fetches of the ical calendar. |
| MEETERS_EVENT_WARNING_TIME_SECONDS | no | 60 | The time in seconds before the next meeting to show the notification. |
| MEETERS_FUTURE_DAYS | no | 1 | The number of future days to show in the calendar view in addition to today. For example, a value of 1 shows today and tomorrow, 2 shows today plus two more days, etc. |
| MEETERS_TODAY_START_HOUR | no | 8 | The start hour of the timeline view (0-23). Events before this hour will not be visible in the timeline. |
| MEETERS_TODAY_END_HOUR | no | 20 | The end hour of the timeline view (0-23). Events after this hour will not be visible in the timeline. |


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
| `thread 'main' panicked at 'Failed to load ayatana-appindicator3 or appindicator3 dynamic library'` | Install the required appindicator library. On Arch Linux, run: `sudo pacman -S libappindicator-gtk3` |