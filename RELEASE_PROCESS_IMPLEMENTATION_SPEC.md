# PR-based release process implementation specification

Status: proposed implementation. This document does not enable automation or authorize a release.

## Objective

Make releasing Meeters a reviewed, repeatable operation: automation collects changes,
proposes a version and changelog in a release PR, and publishes a tested Linux binary
after the maintainer merges that PR. Ordinary development commits must not publish releases.

Use release-plz as a CI tool, not an application dependency. Run in Git-only mode:
Meeters releases are Git tags and GitHub releases; crates.io publication is out of scope.
Keep the existing GTK 4.6 / glibc 2.35 baseline and Ubuntu 22.04 build environment.

## Current repository and migration prerequisites

Repository inspection on 2026-09-09 found:

- `Cargo.toml` declares version `1.7.0`, but local tags extend through `v1.12.0`.
- `.github/workflows/release.yml` runs on GitHub release creation, builds on Ubuntu
  22.04, and uploads an archive with `skx/github-action-publish-binaries@master`.
- `.github/workflows/ci.yml` tests Rust 1.92.0 and stable, including desktop smoke
  tests, GTK dialog tests, and the release binary's glibc requirements.
- The package manifest has `include = ["assets/*"]`.
- Commit subjects are descriptive but do not consistently follow Conventional Commits.

Before enabling the publisher, fetch the remote tags and inspect actual GitHub
releases and tag ancestry. Identify the last release on the intended release line;
do not infer it solely from the highest local tag or the manifest version. Reconcile
`Cargo.toml` and its root package entry in `Cargo.lock` in a reviewed bootstrap PR.
Never move existing tags, reuse an existing version, or rewrite published releases.
The first proposed version must follow the verified baseline; this spec does not
select that version.

Inspect `cargo package --list`. Remove or correct the assets-only include rule so
Rust source and other release-relevant files participate in change detection.
Check source-only, dependency-only, and asset-only commits in a release-plz dry run.
Explicitly verify whether workflow and documentation changes enter the changelog;
configure their inclusion where necessary rather than silently losing them.

## Maintainer experience

1. Development changes merge into `main` and normal CI runs.
2. Automation creates or updates one rolling release PR for Meeters.
3. The PR contains the proposed manifest/lockfile version and `CHANGELOG.md` entry.
4. The maintainer reviews the version, compatibility implications, and release notes.
5. Merging the passing release PR is the release decision. Do not auto-merge it.
6. The publisher validates the exact release commit, creates its `vX.Y.Z` tag and
   GitHub release, and starts artifact preparation.
7. The artifact job verifies and uploads the binary archive and checksum.

Commits merged while a release PR remains open accumulate in that PR. Closing it
must not publish anything. A normal PR must not become a release merely by editing
`Cargo.toml`; reserve the `release-plz-` branch prefix for release preparation.

## Versions and build identity

`Cargo.toml` is the authoritative package version. A release tag must equal `v`
plus that version, and `Cargo.lock` must agree. Use patch versions for compatible
fixes, minor versions for features, and major versions for incompatible changes to
supported configuration, command-line behavior, or the D-Bus interface.

Let the release PR perform version bumps. Do not add a second automatic post-release
`-dev` bump process in the initial implementation. Development builds are identified
by their Git revision; prerelease publishing can be added separately.

The intended About/status UI will display package version, short Git hash, and a
`modified` marker for a dirty build checkout. UI changes are a separate task. Release
build metadata must identify the tagged source commit and must be clean. When that
metadata implementation is added, ensure incremental builds refresh it when HEAD or
working-tree contents change and provide an explicit unknown-revision fallback for
source archives without `.git`. Generated artifacts must not mark source as modified.

## Release-plz configuration

Add `release-plz.toml` using a pinned release-plz version that supports Git-only mode.
Start with these settings and validate their spelling and behavior against that version:

```toml
[workspace]
git_only = true
publish = false
release_always = false
dependencies_update = false
semver_check = false
git_tag_name = "v{{ version }}"
git_release_enable = true
changelog_update = true
pr_labels = ["release"]
```

Keep dependency upgrades in development PRs. The lockfile change in a release PR
should normally be only Meeters' own version. Library API analysis is not a substitute
for reviewing this desktop application's configuration and D-Bus compatibility.

## Changelog policy

Use release-plz's git-cliff integration to produce a checked-in `CHANGELOG.md`.
Configure sections for Features, Fixes, Dependencies, Documentation, and Other changes.
Include links to commits and PRs when available, and compare links between release tags.

Adopt Conventional Commits for new commits or squash-merge subjects:

- `feat: show meeting details` proposes a minor release.
- `fix: preserve room names` proposes a patch release.
- `chore(deps): update compatible dependencies` remains visible under Dependencies.
- `feat!: change configuration format` or a `BREAKING CHANGE:` footer requires review
  of a major version bump and migration instructions.

Do not discard nonconventional historical commits: put unmatched subjects under
Other changes. Never suppress breaking changes. Exclude mechanical release commits
and duplicate merge messages, not meaningful dependency or build changes.

Automation supplies coverage, not editorial judgment. Before merging, condense
repetitive implementation commits and explain user-visible consequences. Verify that
rerunning release-plz preserves edits to the pending release entry. GitHub release
notes must use the approved changelog entry without appending a second duplicate list.

Bootstrap only the unreleased range after the verified last release. Preserve existing
published notes; historical changelog backfilling is optional and must be reviewed.
Do not rewrite commit history to impose a naming convention.

## GitHub authentication and repository configuration

Use a repository-scoped GitHub App, installed only on this repository, with Contents
and Pull requests read/write permissions. Store its App ID as a repository variable
and its private key as an Actions secret. Generate short-lived installation tokens
inside the trusted release workflow. No separate bot server or machine-user account
is required. A repository-scoped fine-grained PAT is an alternative, not the default.

App tokens allow the bot-created release PR to trigger normal PR CI and its release
event to trigger artifact publishing. The default `GITHUB_TOKEN` does not provide
that workflow chaining behavior. Artifact upload itself can use `GITHUB_TOKEN` with
Contents write permission because it need not trigger another workflow.

Enable the repository settings needed for Actions to create PRs. Require existing
CI checks before release PR merge. Keep write credentials out of untrusted PR jobs;
do not use `pull_request_target` to execute contributed code with release secrets.
Pin third-party Actions to reviewed commit SHAs with version comments and pin the
release-plz executable version. Replace the floating upload action with `gh release upload`.

## Workflow implementation

### Release preparation and publication

Add `.github/workflows/release-plz.yml` for pushes to the canonical repository's
`main`, with a guarded manual rerun path for recovery. Fetch full Git history and
tags, disable persisted checkout credentials, and install required native development
libraries if the selected release-plz commands need Cargo metadata or compilation.

Use separate jobs for release-PR maintenance and publication. Serialize release-PR
updates without cancelling an active update. Do not use concurrency cancellation
that can drop a pending publication when another commit reaches `main`.

Before any tag is created, gate publication on passing validation for the exact
release commit: reuse the existing CI jobs as a callable workflow or implement an
explicit check-run gate. PR CI alone does not verify an eventual merge commit.
Do not resolve the build checkout to moving `main` after selecting a release.
Validate the release-plz release-PR association and exact tag target in race tests.

### Artifact publication

Change `.github/workflows/release.yml` to handle `release: published` and a guarded
manual retry accepting an existing release tag. Preserve Ubuntu 22.04 as its runner.
The artifact workflow must:

1. Check out the release tag's exact commit, with Git metadata available.
2. Validate the tag/manifest/lockfile version agreement before compiling.
3. Build with `cargo build --locked --release`; do not update dependencies.
4. Run unit tests, isolated desktop smoke tests, the GTK dialog test, and the glibc
   2.35 check against the tagged source and built binary.
5. Run `cargo audit` with the normal warning policy; fail on reported vulnerabilities.
   Any future exception needs an explicit, reviewed justification.
6. Package the binary and the existing icon assets in a staging directory.
7. Generate a SHA-256 checksum and upload only explicitly named artifacts.

Retain the current `linux-x86` archive suffix initially for download compatibility,
but document that it contains an x86_64 binary. Decide the version-prefix convention
once during implementation and test it; do not accidentally create `vvX.Y.Z` names.

The GitHub release becomes visible before its artifact job completes in this initial
design. Its notes should point to the build run while assets are pending. Surface any
failure on that run. Publishing a draft only after successful uploads is a possible
later enhancement; it requires different orchestration and must not be assumed here.

### Failure and retry behavior

A failure before tagging leaves the release unpublished and must be safely retryable.
A failure after tagging must retry the same release/tag, never create another version
or move the tag. Verify existing tag targets before reusing them. A manual artifact
retry must validate the tag and release and run the same checks as the event-driven job.

Do not silently replace assets already downloaded by users. On retry, skip assets
whose checksums match; fail and request investigation if an existing asset differs.
If the source needs a fix, prepare a new patch release. Test how the pinned release-plz
version recovers from a tag existing without its corresponding GitHub release.

## Files to add or modify

| File | Purpose |
|---|---|
| `release-plz.toml` | Version, tag, release PR, and changelog policy |
| `cliff.toml` (optional) | Changelog configuration if kept outside release-plz.toml |
| `CHANGELOG.md` | Reviewed release history and pending release entry |
| `.github/workflows/release-plz.yml` | Release PR maintenance and guarded publisher |
| `.github/workflows/release.yml` | Exact-tag artifact build, validation, upload, retry |
| `.github/workflows/ci.yml` | Reusable validation and any new release-PR checks |
| `Cargo.toml`, `Cargo.lock` | Bootstrap version reconciliation and package file coverage |
| `README.md` | Current release procedure, commit conventions, and recovery commands |
| `tests/` or `scripts/` | Small version-validation helper and focused tests if needed |

Document current behavior in the README rather than the history of the migration.
Do not add release tooling to Meeters' runtime or build dependencies.

## Acceptance and rollout

Implementation is complete when:

- Dry-run output selects the correct existing release baseline and a new, unused version.
- Source-only, asset-only, and dependency-only changes produce a release proposal.
- Nonconventional commits remain visible and repeated runs do not duplicate entries.
- Normal development pushes and closed release PRs cannot publish a release.
- A bot-created release PR runs required CI, and a failed check blocks its merge.
- A merged release PR publishes its intended commit even if later changes reach main.
- A deliberately mismatched tag/version fails before artifact build or upload.
- The App-created GitHub release triggers the Ubuntu 22.04 artifact workflow.
- Build, tests, desktop checks, audit, and glibc checks pass before assets are attached.
- Retry tests cover duplicate events, partial uploads, and tag-without-release recovery.
- No crates.io token is required and no crate publication occurs.

First land the configuration and validated changelog generation with publication
disabled. Exercise workflow chaining in a disposable repository with the same App
permissions. Review the first real release PR and enable publication in a separate,
reviewed change only after bootstrap and acceptance checks pass. These are rollout
requirements for the implementation, not actions performed by writing this spec.

## References

Consult the documentation for the pinned versions during implementation:

- [Release-plz GitHub quickstart](https://release-plz.dev/docs/github/quickstart)
- [Git-only mode and configuration](https://release-plz.dev/docs/config)
- [GitHub token and workflow chaining](https://release-plz.dev/docs/github/token)
- [Changelog generation](https://release-plz.dev/docs/changelog)
- [Editing release PRs and historical commit handling](https://release-plz.dev/docs/faq)
