This program is mean to be a Linux graphical utility that lives in the tray (as an app indicator), watches a configured ical calendar file and will notify shortly before a meeting begins with the added (and crucial) feature of extracting any online meeting URLs from the invitation and allowing one click access.

# TODO
1. remove duplicate events, I appear to have some after expanding with rules: how to identify duplicates? Maybe they are also caused by the mix in timezones: the rrule library can not parse the long TZIDs and therefore makes everything UTC and now I have duplicate events in CET and UTC. Maybe this is resolved when we get this fixed in rrule
1. Identify and parse meeting URLs from the summary and description. Is just a string for the meeting URL sufficient? Shouldn't we directly put it into some kind of object that identifies the type of meeting (like Zoom)?
1. Try to use anyhow and see if that makes things better
1. Try to figure out what sort of rust construct exists for not implemented yet any type
