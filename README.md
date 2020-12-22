This program is mean to be a Linux graphical utility that lives in the tray (as an app indicator), watches a configured ical calendar file and will notify shortly before a meeting begins with the added (and crucial) feature of extracting any online meeting URLs from the invitation and allowing one click access.

# TODO

1. Do manual removal of altered or canceled recurring events.  See https://github.com/fmeringdal/rust_rrule/issues/7 for a description and a link on how to maybe do this. I think for me it will be totally sufficient to look at all recurrence instances for this day and then look through all the "normal" events, identify those that refer to an instance of a recurrence and check whether it is to referring to the recurrence events. I think the exceptions can be easily identified with some of their properties.
1. remove duplicate events, I appear to have some after expanding with rules: how to identify duplicates? Maybe they are also caused by the mix in timezones: the rrule library can not parse the long TZIDs and therefore makes everything UTC and now I have duplicate events in CET and UTC. Maybe this is resolved when we get this fixed in rrule.
1. Fix the escaped commas, they appear to be prefixed with "\".
1. Try to use `anyhow` and see if that makes things better.
1. Try to figure out what sort of rust construct exists for not implemented yet any type.
