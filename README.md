This program is mean to be a Linux graphical utility that lives in the tray (as an app indicator), watches a configured ical calendar file and will notify shortly before a meeting begins with the added (and crucial) feature of extracting any online meeting URLs from the invitation and allowing one click access.

# TODO

1. Remove duplicate events, I appear to have some after expanding with rules: how to identify duplicates? Maybe they are also caused by the mix in timezones: the rrule library can not parse the long TZIDs and therefore makes everything UTC and now I have duplicate events in CET and UTC. Maybe this is resolved when we get this fixed in rrule.
1. Fix timezone handling: both in parsing normal begin and end times I need to look at the TZID string and map to the correct timezone instead of defaulting to Berlin and I need to make sure I convert to the local timezone before feeding into rrule so that we get the correct dates. I am already getting UTC DTSTARTs in meetings from some developers
1. Fix the escaped commas, they appear to be prefixed with "\".
1. Try to use `anyhow` and see if that makes things better.
1. Try to figure out what sort of rust construct exists for not implemented yet any type.

# Dealing With Changes to Recurring Events

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

