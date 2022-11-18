- add an icon state for no events
- reflect ical fetching error in error state in icon
- investigate adding secrets support for basic authenticated URLs

## Add optional support for opening zoom meeting directly in zoom

Some experiments show that zoom still supports the "zoommgt" links.

These are of the form `zoommtg://zoom.us/join?confno=123456789&pwd=xxxx&zc=0&browser=chrome&uname=Betty`.

We should be able to convert a link like `https://<company>.zoom.us/j/<meetingid>?pwd=<password>` into that one and then use `launch_default_for_uri` from ` gio::AppInfo` to launch this.

This feature should be optional and enabled through a configuration parameter and if we fail to convert the URL we should fallback to the `show_uri` method. 
