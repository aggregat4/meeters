# EWS Basic Auth Keyring Implementation Spec

## Goal

Add an optional Exchange Web Services calendar source for on-prem Exchange installations.
The first implementation should authenticate with Basic auth over HTTPS and store the Exchange
password in the user's desktop keyring/wallet instead of in the meeters config file.

The existing published ICS flow must remain the default and continue to work unchanged.

## EWS Endpoint Guidance

Configure the public/client-facing EWS endpoint directly:

```text
https://mail.example.com/EWS/Exchange.asmx
```

The implementation should not fetch the WSDL at runtime. Some on-prem Exchange deployments
redirect `?wsdl` requests to internal backend hosts or ports that are not reachable from the
client network. The app does not need the WSDL for normal operation; it should send known EWS
SOAP requests directly to `Exchange.asmx`.

For setup/debugging, validate the actual SOAP endpoint with a low-impact EWS request such as
`GetServerTimeZones` instead of validating by downloading `?wsdl`.

Implementation consequence: use the configured `Exchange.asmx` endpoint for all EWS SOAP
requests and avoid WSDL discovery in the first version.

## Non-Goals

- Do not replace the ICS code path.
- Do not implement Microsoft Graph.
- Do not implement OAuth in the first version.
- Do not implement Exchange Autodiscover in the first version.
- Do not implement incremental EWS sync in the first version.
- Do not store Exchange passwords in `meeters_config.env`.

## User-Facing Configuration

Introduce a calendar source selector:

```env
MEETERS_CALENDAR_SOURCE=ics
```

Allowed values:

- `ics`: existing behavior. This is the default.
- `ews`: fetch calendar data from Exchange Web Services.

For ICS:

```env
MEETERS_ICAL_URL=https://example.com/calendar.ics
```

For EWS:

```env
MEETERS_CALENDAR_SOURCE=ews
MEETERS_EWS_URL=https://mail.example.com/EWS/Exchange.asmx
MEETERS_EWS_USER=user@example.com
```

`MEETERS_EWS_USER` must be an email/UPN-style value such as `user@example.com`.
Domain-qualified usernames such as `DOMAIN\user` are not supported in the first version.

The password should be stored outside the config file under a stable keyring entry:

```text
service: net.aggregat4.meeters.exchange
user:    value of MEETERS_EWS_USER
```

## Credential UX

Initial version:

1. On startup with `MEETERS_CALENDAR_SOURCE=ews`, look up the password in the keyring.
2. If no password exists, ask the user for it with a GTK password dialog.
3. Store the entered password in the desktop keyring/wallet.
4. Retry the calendar refresh after the password is stored.

The password dialog should:

- Identify the configured EWS user and endpoint.
- Use a hidden password entry.
- Make storing in the desktop keyring explicit in the dialog text.
- Offer cancel. Cancel should leave the refresh in a failed state with a clear message.

If EWS returns an authentication failure and a password is already stored, the app should offer
to replace the stored password.

## Keyring Backend

Use the FreeDesktop Secret Service API on Linux. This is the common compatibility layer for:

- GNOME Keyring
- KDE Wallet, when Secret Service support is enabled
- KeePassXC, when Secret Service integration is enabled

Recommended Rust crate:

- `keyring` with the synchronous Secret Service backend.

Recommended dependency shape:

```toml
keyring = { version = "3.6", default-features = false, features = ["sync-secret-service", "crypto-rust"] }
```

Reasons:

- The app is currently synchronous outside the GTK main loop.
- `keyring::Entry` provides a small `set_password` / `get_password` API.
- It avoids introducing a Tokio runtime only for credential storage.
- It still uses the FreeDesktop Secret Service backend, which is the KDE/GNOME portability
  target for this feature.

Because meeters is currently Linux/GTK/appindicator-focused, using Secret Service through the
`keyring` crate is acceptable. Portability across GNOME and KDE is good when the user has a
Secret Service provider running. Headless sessions and locked wallets are expected failure cases.

## EWS HTTP Client

The existing ICS fetch uses `ureq`. The EWS path should use `reqwest` for straightforward Basic
auth support.

Reasons:

- The existing refresh loop is synchronous and runs on a worker thread.
- Basic auth over HTTPS has the same local credential-storage posture as NTLM for this app,
  because the app still needs the real Exchange password from the desktop keyring.
- Avoiding NTLM keeps the first implementation smaller and avoids libcurl build-feature issues.
- `reqwest::blocking` fits the current synchronous refresh loop.

The EWS client must:

- Set a global/request timeout.
- Disable following redirects for EWS SOAP requests unless there is a specific reason.
- Send all SOAP requests to the configured `MEETERS_EWS_URL`.
- Avoid logging passwords, authorization headers, or full request headers.

## Calendar Fetch Strategy

First version should poll the visible window, mirroring the current ICS behavior:

- Start: local start of today.
- End: local end of `today + MEETERS_FUTURE_DAYS`.

Use EWS `FindItem` with `CalendarView`.

Required request properties:

- `BaseShape=IdOnly` plus selected fields, or a suitable shape that includes the required
  calendar fields.
- Calendar folder: distinguished folder id `calendar`.
- Traversal: `Shallow`.
- Calendar view start and end timestamps.

Required event fields:

- Subject -> `Event.summary`
- Start -> `Event.start_timestamp`
- End -> `Event.end_timestamp`
- Location -> `Event.location`
- IsAllDayEvent -> `Event.all_day`
- Online meeting URL extraction from body/location should reuse existing logic where possible.

Body/description handling:

- `FindItem` should be used to discover calendar occurrences and item IDs.
- A follow-up `GetItem` request should retrieve body content for each item that will be shown.
- Request text body content with EWS `BodyType=Text` where supported.
- If Exchange still returns HTML, convert it to plain text before storing it in
  `Event.description`.
- Use `Event.description` for the existing timeline tooltip behavior.
- Reuse the existing Zoom URL extraction order as closely as possible:
  location, summary, description.

This differs from the ICS path only in where the description comes from. The ICS path reads and
unescapes the `DESCRIPTION` property directly; the EWS path needs `GetItem` because `FindItem`
does not return full body content.

Recurring meetings:

EWS `CalendarView` expands recurring calendar items into occurrences for the requested time
range. The first version should rely on Exchange for recurrence expansion instead of porting the
current ICS recurrence logic into the EWS path.

## Code Structure

Proposed modules:

```text
src/calendar_source.rs
src/ews.rs
src/secrets.rs
```

`calendar_source.rs`:

- Defines `CalendarSourceConfig`.
- Defines a fetch function or trait that returns `Vec<Event>`.
- Keeps `main.rs` from knowing whether events came from ICS or EWS.

`ews.rs`:

- Builds SOAP XML requests.
- Sends EWS requests using Basic auth over HTTPS.
- Parses EWS SOAP responses.
- Converts EWS calendar items into `Event`.

`secrets.rs`:

- Looks up and stores the EWS password in Secret Service.
- Contains all keyring-specific labels and attributes.
- Returns actionable errors for missing, locked, or unavailable keyrings.

`config.rs`:

- Parse `MEETERS_CALENDAR_SOURCE`.
- Keep `MEETERS_ICAL_URL` required only when source is `ics`.
- Require `MEETERS_EWS_URL` and `MEETERS_EWS_USER` only when source is `ews`.
- Preserve the existing public config behavior for current users.

`main.rs`:

- Replace direct `get_ical(...).and_then(extract_events...)` call with a calendar-source fetch.
- Keep notification, filtering, and GUI update logic unchanged.

## Error Handling

Refresh errors should be visible in the existing refresh log.

Expected EWS error cases:

- Missing keyring entry.
- Secret Service unavailable.
- Wallet locked.
- HTTP authentication failure.
- Exchange SOAP fault.
- Malformed/unexpected SOAP response.
- Timeout or network failure.

Password-related errors should not include the password or authentication headers.

## Testing Plan

Unit tests:

- Config parsing for `ics` default behavior.
- Config parsing for valid/invalid `ews` settings.
- EWS XML response parsing from fixture files.
- EWS SOAP fault parsing.
- Event conversion, including all-day events and online meeting URL extraction.

Manual smoke tests:

1. Start with `MEETERS_CALENDAR_SOURCE=ews` and no stored password.
2. Confirm the app prompts for the EWS password.
3. Confirm the password is stored in the desktop keyring/wallet.
4. Confirm refresh log shows successful EWS fetch.
5. Confirm current and future-day meetings appear in the tray/window.
6. Confirm meeting notifications still fire.
7. Confirm removing/renaming the keyring entry prompts again.
8. Confirm a meeting URL in the Exchange body is detected and opens correctly.

Integration tests against the real Exchange server should not be part of normal CI.

## Rollout Plan

Phase 1:

- Add config model and source abstraction.
- Add UI password prompting backed by Secret Service storage.
- Add EWS `FindItem` polling plus `GetItem` body retrieval for the current visible date window.
- Keep ICS as default.

Phase 2:

- Improve XML fixtures and parsing coverage.
- Add optional GTK password setup dialog if useful.
- Evaluate Kerberos/Negotiate support if Basic auth becomes unavailable.

Phase 3:

- Consider `SyncFolderItems` for incremental sync.
- Consider Autodiscover.
- Consider OAuth if the Exchange environment supports it and the app registration story is clear.

## Open Questions

- Should failed authentication always trigger a replacement-password dialog, or should the app
  rate-limit that prompt to avoid nagging when the server is temporarily unavailable?
- Should the app fetch bodies for every visible event immediately, or fetch bodies lazily only
  after a `FindItem` result suggests the event may need a tooltip/meeting URL?
