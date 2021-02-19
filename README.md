This is graphical (GTK) utility for Linux that lives in the tray as an app indicator, watches a configured ical calendar file URL and will notify shortly before a meeting begins. It allows you to directly open any (Zoom) embedded meeting URL with a single click from either the popup menu or the notification.

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

# TODO

* Colorize the popup menu appointments by past/current/upcoming
* Try to use `anyhow` and see if that makes error handling better.

# Implementation Details

## Dealing With Changes to Recurring Events

Some background discussion: <https://icalevents.com/4437-correct-handling-of-uid-recurrence-id-sequence/>.

A recurring event can be identified by it having an RRULE attribute.

Each recurring event also has:
* a SEQUENCE attribute
* a UID Attribute

Each event that indicates a change to the recurring event also has the same UID and perhaps also the same SEQUENCE, but it is unclear how or if that is relevant.

This modifying event also has a RECURRENCE-ID which can be used to identify the instance of the recurrence that has to be modified.

So the algorithm to correct recurring instances looks like this:
* Create a map of all modifying event instances and index that map by UID
* For each recurring event identify all the instances (up to a certain date perhaps), mark those instances as part of a recurrence
* For each recurring ocurrence check whether there is a modifying instance with the same UID and a matching RECURRENCE-ID (this is a date time value (always?) that has to match the starting time of the occurence)
* If the occurence has a modifying event: replace the occurence with the modification

