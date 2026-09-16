# Timezone library evaluation for Meeters

Evaluated 2026-09-15. Production code and dependencies are unchanged.

### Follow-up decision — 2026-09-16

Retain Chrono and the existing custom timezone implementation for now. Update the
application to Chrono 0.4.45 (already resolved) and chrono-tz 0.10.4, sharing the
production timezone crate with upstream rrule. Defer Jiff/tz-rs integration until
changes in rrule's dependencies, Chrono maintenance, or application requirements
justify revisiting it. The evaluation below records the pre-update baseline;
its isolated harness intentionally retains chrono-tz 0.5.3 for comparison.
The test-only rrule fork has also been retired; production and application tests
now use one chrono-tz version. The standalone evaluation alone retains the old
version to reproduce the historical comparison.

## Recommendation

**Jiff is my preferred long-term direction. tz-rs is the strongest smaller replacement for the custom timezone arithmetic.** Both passed the executable custom-zone evaluation. Jiff's richer civil-time and ambiguity APIs justify a further integration experiment; its custom-zone construction is less convenient than tz-rs, and that is a real cost for this application.

Before a larger migration, **update the direct chrono-tz dependency and establish a timezone-data update policy**. The current 0.5.3 data gives different offsets from current system data in four of eight sampled regions. This is an existing correctness concern independent of which library eventually wins.

I would next trial Jiff behind a small application-owned timezone interface, keeping Chrono at existing boundaries. If maintaining a TZif encoder proves disproportionate, use tz-rs behind that same interface. There is no reason to migrate through both libraries or rewrite the entire application immediately.

## Plan and evaluation scope

The evaluation followed four steps:

1. Inspect Meeters' actual timezone, recurrence, and event representations.
2. Screen relevant Rust timezone libraries and a calendar-level alternative against those requirements.
3. Build and run the strongest candidates using the existing VTIMEZONE compiler and approved calendar fixtures.
4. Separate library behavior, timezone database freshness, and integration costs before recommending a direction.

The important requirements are arbitrary Exchange VTIMEZONE definitions; historical and RDATE transitions; fractional offsets and non-hour DST; explicit handling of nonexistent and repeated local times; deterministic behavior independent of the host timezone; and compatibility with the upstream recurrence adapter.

This is a working backend feasibility evaluation, **not a completed end-to-end migration**. Jiff, tz-rs, and tzfile were executed; the remaining options received API/documentation review. “All relevant” here means plausible Rust options for this application plus the established calendar-native C alternative, rather than every date/time crate.

## Why the current boundary matters

Meeters already has two distinct responsibilities:

- `timezones.rs` interprets VTIMEZONE observances and expands recurrence into UTC transitions.
- `custom_timezone.rs` resolves offsets and local times against that transition table.

Replacing the second does not remove the first. Neither Jiff nor tz-rs directly interprets Exchange VTIMEZONE. Mapping an Exchange identifier to an IANA name would lose the embedded calendar's authority and potentially its history.

The existing compiler can therefore remain while a library takes over transition lookup and ambiguity detection. Upstream rrule can continue receiving civil fields through the current UTC-carrier adapter. Even a broader Jiff migration would leave Chrono transitively present through rrule.

## Candidate comparison

| Option | Custom Exchange zone route | Main trade-off | Assessment |
|---|---|---|---|
| Current CustomTz + updated chrono-tz | Keep existing compiler and arithmetic | Least migration; retain custom lookup, gap handling, and maintenance | Sensible baseline, including an immediate data refresh |
| **Jiff 0.2.37** | Compile transitions to TZif, load with `TimeZone::tzif` | Rich date/time API and explicit ambiguity; must maintain a TZif construction boundary | Preferred long-term candidate; executed |
| **tz-rs 0.7.3** | Construct transition and local-type vectors directly | Excellent fit for existing compiler; low-level API and application-owned policy | Preferred narrow replacement; executed |
| tzfile 0.1.3 | Compile TZif, use its Chrono timezone implementation | Easy Chrono bridge, but future TZif rules unsupported | Reject for general named-zone use; executed |
| time + time-tz 2.0.0 | Implement its public TimeZone trait | Customizable, but custom resolution remains your responsibility; another date/time migration | No compelling advantage here; API review |
| temporal_rs 0.2.6 + timezone_provider | Implement a custom provider | Temporal semantics and provider flexibility; substantial integration surface | Consider if Temporal compatibility becomes a goal; API review |
| libical | Use a VTIMEZONE component as a timezone | Can replace more calendar logic; C FFI, packaging, and wider semantic migration | Worth a separate calendar-engine study, not this narrow replacement |
| ICU4X time/timezone facilities | Identity, localization, and external offset integration | Useful for internationalized presentation; different responsibility | Complementary, not the selected transition engine |

### Jiff: strong APIs, awkward custom construction

Jiff accepts runtime TZif bytes and has explicit gap/fold APIs. Its `compatible()` disambiguation matched the application's first-occurrence fold policy and pre-transition-offset gap policy in the probes. Cheap shared timezone values are also preferable to repeatedly cloning Meeters' transition vectors. There is no public arbitrary-transition builder comparable to tz-rs: custom construction goes through TZif. A POSIX rule alone cannot represent arbitrary historical Exchange observances. [TimeZone API](https://docs.rs/jiff/0.2.37/jiff/tz/struct.TimeZone.html), [design](https://docs.rs/jiff/latest/jiff/_documentation/design/index.html).

An incremental integration can convert civil fields and timestamps at the boundary; adopting `Zoned` throughout the event model is optional subsequent work. Keep calendar-scoped custom-zone identity separately if values must be serialized and reloaded: a synthetic zone name does not distribute its definition to another process.

Choose an explicit timezone-data strategy. System data receives OS updates; bundled data is reproducible only when its version is controlled and updated. Tests should load pinned bytes or compiled custom tables. Avoid silently treating failed system-zone discovery as UTC; Jiff provides fallible discovery APIs. [Platform guidance](https://docs.rs/jiff/latest/jiff/_documentation/platform/index.html).

### tz-rs: the closest fit for today's code

`TimeZone::new` accepts transitions, local time types, leap-second entries, and an optional continuation rule. That maps naturally to the existing compiler without inventing a binary format. The library also reads TZif. It supplies lower-level offset and local-time operations; Meeters must retain the zone registry, Chrono conversions, and calendar-specific resolution policy. [TimeZone API](https://docs.rs/tz-rs/0.7.3/tz/struct.TimeZone.html), [crate documentation](https://docs.rs/tz-rs/0.7.3/tz/).

Two prototype findings matter:

- A finite transition list without a continuation rule returned `NoAvailableLocalTimeType` at/after its final transition. The adapter now supplies a fixed final rule to match current CustomTz behavior. Production should instead enforce the compiler's valid date window and extend it when necessary.
- A skipped local time is represented with transition boundaries. Taking an “earliest” boundary is not equivalent to interpreting the requested wall time using the preceding offset. The adapter explicitly computes that policy, preserving minutes within the gap.

This is the option I would choose if the goal is specifically to delete hand-maintained timezone arithmetic while preserving the current Chrono model. It provides less motivation than Jiff for a wider date/time redesign.

### Chrono and chrono-tz: viable baseline, separate data issue

Updating chrono-tz preserves existing event and display types. Newer chrono-tz also exposes `GapInfo`, useful for IANA-zone gap handling; it does not automatically provide the same facility for the application's separate CustomTz type. [GapInfo](https://docs.rs/chrono-tz/0.10.4/chrono_tz/struct.GapInfo.html).

There is a maintenance concern, but it should be described accurately: a maintainer opened a January 2026 discussion expressing an inclination to wind down Chrono/chrono-tz maintenance and discussing succession. That is evidence to consider, **not proof that the projects are abandoned or formally deprecated**. [Maintainer discussion](https://github.com/chronotope/chrono/issues/1768).

### Why the other options did not make the shortlist

tzfile provides a convenient Chrono bridge, including shared wrappers, but explicitly does not implement the POSIX footer that continues TZif rules into the future. The 2050 probes exposed this limitation. It also needs the same custom TZif construction work as Jiff, with fewer benefits. [tzfile documentation](https://docs.rs/tzfile/0.1.3/tzfile/struct.Tz.html).

time-tz has a public timezone trait: it is not restricted to IANA zones. However, implementing that trait means providing local/UTC offset resolution, so it does not itself replace the custom transition algorithm. Its IANA/Windows integration is useful, but Meeters would still need a Chrono bridge for recurrence. [time-tz](https://docs.rs/time-tz/2.0.0/time_tz/), [custom trait](https://docs.rs/time-tz/2.0.0/time_tz/trait.TimeZone.html).

temporal_rs targets the ECMAScript Temporal model and supports custom timezone providers. This is a meaningful extension mechanism, but provider implementation and a broader model conversion are more work than this application presently needs. It was not compiled in this evaluation. [temporal_rs](https://docs.rs/crate/temporal_rs/0.2.6), [provider APIs](https://docs.rs/temporal_rs/latest/temporal_rs/provider/index.html).

libical directly accepts timezone components and exposes offset calculation, potentially outsourcing VTIMEZONE interpretation too. That deserves comparison if we later reconsider the entire calendar engine. It was not built here: libical was not available through pkg-config, and introducing FFI would widen the experiment considerably. [Timezone component API](https://libical.github.io/libical/v4/apidocs/icaltimezone_8h_source.html).

ICU4X is useful for timezone identity and localized naming. Its zone documentation directs users to libraries such as Jiff or chrono-tz for timezone offset information. It does not solve this particular replacement by itself. [ICU4X zone documentation](https://docs.rs/icu/latest/icu/time/zone/index.html).

## Executed results

### Custom zones

The harness imports the real VTIMEZONE compiler from the source tree. It converts its tables into Jiff TZif, tz-rs transitions, and tzfile TZif. An independent linear table lookup and offset-candidate enumeration check the backend arithmetic; they do not independently validate the shared VTIMEZONE compiler.

Across **12 zone instances**, including the sanitized Exchange calendar and a constructed negative-offset zone with a 30-minute change:

- **349 UTC samples passed** across CustomTz, Jiff, tz-rs, and tzfile.
- **943 local-time candidate samples passed**, including transition boundaries and available fixture event timestamps.
- **31 gap-policy samples passed** for Jiff and the explicit tz-rs policy mapping.
- Correctness output was identical under `TZ=UTC`, `Europe/Berlin`, `America/New_York`, and `Asia/Kolkata`.

Zone instances are not 12 independent real-world calendars. Fixtures reuse timezone shapes. Event timestamp probes cover matching DTSTART/DTEND/EXDATE/RECURRENCE-ID/RDATE properties; this does not replay every generated occurrence through a replaced production backend. All-day DATE fields are deliberately not treated as timezone instants.

### Named zones and data freshness

The test samples eight IANA zones at four dates each, including 2050. Jiff and tz-rs agree on **all 32 samples using identical system TZif bytes**. This establishes agreement, not an independent oracle for future legislation.

| Comparison | Observed result |
|---|---|
| chrono-tz 0.5.3 vs system TZif | 10 differing samples across Apia, Mexico City, Almaty, Cairo |
| chrono-tz 0.10.4 vs system TZif | No differences in these 32 samples |
| tzfile vs Jiff/tz-rs using identical bytes | Four differing 2050 samples: Berlin, New York, Lord Howe, Cairo |

The installed system database is **2026c**; chrono-tz 0.10.4 embeds **2025b**; the old 0.5.3 vendored data's NEWS identifies **2020a**. The old-version differences are consistent with stale rules. The absence of differences for 0.10.4 in this small sample does not make its database current. Future dates compare extrapolation of current rules, not predictions of government decisions.

### Performance and compatibility

A single release microbenchmark of 100,000 repeated UTC-offset lookups measured approximately **26 ns/op for CustomTz, 4 for Jiff, 4 for tz-rs, and 5 for tzfile**. Current CustomTz clones its transition table during lookup. These figures use a small table and one repeated timestamp; they exclude construction, local ambiguity resolution, memory footprint, and whole-calendar processing. They support removing avoidable cloning, not an application speedup claim or a ranking between Jiff and tz-rs.

Test environment: Linux x86_64, Rust 1.98.1. Downloaded package metadata declares Rust 1.70 for Jiff and 1.85 for tz-rs, both below this application's stated 1.92 requirement. The application's minimum toolchain was not tested. Jiff is Unlicense OR MIT; tz-rs is MIT OR Apache-2.0. The isolated lockfile resolves its own transitive versions, including Chrono 0.4.45; this was not a production lockfile compatibility test.

## What must happen before switching

1. **Refresh IANA data independently.** Upgrade direct chrono-tz, run the deterministic suite, and decide how bundled or OS timezone updates reach releases.
2. **Introduce a small timezone interface.** It should expose offset-at-instant and local-time resolution, preserve calendar-scoped identity, and carry the custom transition table's validity window.
3. **Trial Jiff through that interface.** Keep existing recurrence and display boundaries. Harden custom TZif encoding: deduplicate types, preserve relevant metadata, validate limits, test round trips and historical boundaries, and explicitly handle continuation/validity. The evaluation encoder is intentionally incomplete.
4. **Replay the full parser/recurrence fixtures with both backends.** Compare ordered event instants, exceptions, moved instances, all-day behavior, and errors across host timezones. Add exact named-zone gap/fold expectations using pinned data; the current IANA experiment only checks offsets at selected instants.
5. **Gate adoption on those results.** Extend coverage to large jumps, densely spaced transitions, date-range limits, malformed input, and compiler-horizon extension. Measure whole-calendar costs only if performance affects the choice.

The evaluation encoder writes a minimal TZif v2 representation with a reduced 32-bit block, synthetic abbreviations, no DST metadata, no leap seconds, no future footer, and a small type-count limit. It is suitable for these probes, not production. Neither backend can recover missing transitions after a finite compilation horizon; silently extending the final offset is especially dangerous for zones whose DST continues.

**Decision:** the earlier custom-zone concern is valid, but it does not rule out Jiff. The experiment establishes a viable bridge. I recommend exploring that bridge next, with tz-rs as the lower-effort choice if the desired scope remains strictly the current CustomTz implementation.

## Reproduce

From the repository root:

```sh
cargo run --release --manifest-path evaluations/timezones/Cargo.toml --locked
```

The program runs assertions and exits unsuccessfully on a custom-backend disagreement. Named-zone differences are reported rather than asserted away, because database versions and tzfile's known limitation intentionally differ. It requires system zoneinfo files under `/usr/share/zoneinfo`; those files are not pinned by Cargo.lock, so IANA results can change with OS updates. The approved source-tree fixtures are used; the private raw calendar is never read.

The harness uses `cargo run`, not Rust's test harness, because it imports production modules without their application-wide unit-test dependencies. See [source](src/main.rs), [locked dependencies](Cargo.lock), and [recorded output](results.txt). No runtime replacement or production test coverage is claimed by merely adding this experiment.
