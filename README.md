This is a graphical (GTK) utility for Linux that lives in the tray as an app indicator, watches a configured ical calendar file URL and will notify shortly before a meeting begins. It allows you to directly open any (Zoom) embedded meeting URL with a single click from either the popup menu or the notification.

# Building

1. Clone repo
1. `cargo b`

# Installation

You can drop the meeters binary anywhere. The tarball includes 2 icons that will be used when they are located next to the meeters binary. If not the program will default to a "new appointment" icon.

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

# Troubleshooting

## `thread 'main' panicked at 'Failed to load ayatana-appindicator3 or appindicator3 dynamic library`

If you get an error regarding a missing appindicator library, you need to install it first. On Arch Linux this is done with:

```
sudo pacman -S libappindicator-gtk3
```
