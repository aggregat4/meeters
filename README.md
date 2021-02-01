This program is mean to be a Linux graphical utility that lives in the tray (as an app indicator), watches a configured ical calendar file and will notify shortly before a meeting begins with the added (and crucial) feature of extracting any online meeting URLs from the invitation and allowing one click access.

# TODO

* Colorize the popup menu appointments by past/current/upcoming
* Custom notifications 1 minute before a meeting with a clickable link (discard when? on click and after some time? Can I use actual notifications?)
* Fix the escaped commas, they appear to be prefixed with "\".
* Try to use `anyhow` and see if that makes error handling better.

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

