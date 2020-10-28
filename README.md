This program is mean to be a Linux graphical utility that lives in the tray (as an app indicator), watches a configured ical calendar file and will notify shortly before a meeting begins with the added (and crucial) feature of extracting any online meeting URLs from the invitation and allowing one click access.

# TODO
1. Recurring events, the plan: 
  - write a function to convert an IcalEvent into a string again
  - use rrule as a library to parse this string into a vec of occurences when we know that it contains a recurrence (see <https://docs.rs/rrule/0.2.8/rrule/>)
1. Identify and parse meeting URLs from the summary and description. Is just a string for the meeting URL sufficient? Shouldn't we directly put it into some kind of object that identifies the type of meeting (like Zoom)?
1. Try to use anyhow and see if that makes things better
1. Try to figure out what sort of rust construct exists for not implemented yet any type
