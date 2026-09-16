# Recurrence engine comparison

## Status — 2026-09-16

The migration comparison below is a historical record. The test-only fork has
now been removed, along with its chrono-tz 0.5.3 dependency. Tests exercise
upstream against explicit calendar and timezone expectations; the five legacy
recurrence inputs assert occurrence counts and first/last dates. Invalid BYDAY
still has an error regression test. The former fork-only smoke tests were
redundant with those adapter tests and have been removed.

Production and tests now share chrono-tz 0.10.4. The commands below run the
retained regression suites, but no longer perform live fork comparisons; the
old-version tree command is no longer applicable. Historical comparisons remain
available in Git history.

## Historical result

The fork and adapted upstream produce identical outputs for the calendar
compatibility fixtures, including the sanitized Exchange export. This covers
occurrence order and count, start/end instants, local representations, all-day
flags, titles and metadata after exclusions and moved-instance processing.
Parsed custom timezone transition tables are also compared directly by the
transition fixtures. Explicit observance UNTIL boundary tests run both engines.

One known difference on malformed input is recorded separately:

| Input | Fork | Upstream |
| --- | --- | --- |
| `BYDAY=INVALID` | Panics in `parse_weekday` | Returns a parse error |
| Invalid FREQ | Returns a parse error | Returns a parse error |
| Invalid DTSTART | Returns a parse error | Returns a parse error |

Error wording from the libraries is not a compatibility requirement. The
malformed-BYDAY test catches the panic solely to characterize this difference;
production now uses upstream and returns an error for this input. The panic
remains only in the test-only reference fork.

## Versions and scope

- Production: registry `rrule` **0.14.0**, exactly pinned behind the adapter.
- Test-only reference: fork `rrule` 0.5.10 (alias `rrule_fork`), revision
  `44df30566e6b6ab7e1c98f59c03e413ec996a570`.
- Production selects `Engine::Upstream`; the Fork variant only compiles under
  `cfg(test)`. Meeters continues to resolve zones with its existing chrono-tz
  and custom timezone code; upstream's newer chrono-tz is used only for the
  adapter's UTC field representation.
- Synthetic fixtures evaluate 2024; the real Exchange export evaluates 2026.
- Five historical recurrence regression inputs are compared in addition to the
  end-to-end calendar contracts.

## Adapter decisions

`src/recurrence.rs` owns recurrence expansion for both events and timezone
observances. Meeters still owns embedded timezone definitions, offset arithmetic,
DST gap/fold policy, EXDATE matching and RECURRENCE-ID overrides. The selected
engine is passed explicitly, without process-global or thread-local switches.

The upstream adapter encodes already-normalized calendar fields as UTC. This
prevents upstream's default local timezone from changing floating expansion.
These UTC values are field carriers; for local events they are not the meeting's
actual UTC instant. DATE values become midnight, and UNTIL uses the same field
representation. This also preserves the fork's handling of the legacy input
that labels a datetime as VALUE=DATE. No arbitrary TZID mapping or new IANA
rules are used for resolving events by the recurrence engine.

The adapter iterates only to the exclusive end year used by the application,
rather than calling `.all()` before filtering. Both engines are tested on an
unending yearly rule. This is a year boundary, not a general CPU/memory budget:
very dense or pathological rules still need separate resource-limit work.

## Reproduce

```sh
cargo test --locked calendar_compatibility
cargo test --locked timezones::tests
for zone in UTC Europe/Berlin America/New_York Asia/Kolkata; do
  TZ="$zone" cargo test --locked calendar_compatibility
  TZ="$zone" cargo test --locked timezones::tests
done
cargo build --locked --release
cargo tree --locked --edges normal -i rrule@0.5.10
```

The last command should report nothing: the fork is absent from the production
dependency tree. Existing CI runs both test groups under all four host timezones.
The real-calendar test additionally compares four display timezones.

## What this does not establish

This is evidence for replacing the recurrence engine *behind this adapter*,
not for replacing Meeters' timezone handling or feeding raw Exchange calendars
directly into upstream. Matching engines can share a bug in the surrounding code;
the handwritten synthetic assertions remain important.

Event RDATE, floating recurrence support, broader recurrence-rule combinations,
pathological input limits and further real-world providers remain outside this
comparison. See README.md for the remaining calendar coverage. The explicit expected-output contracts now guard production upstream behavior.
