# Jigsaw: Verifies Plugin Compatibility with a Host (Core)

This guide covers `backhopper jigsaw`:

 * [Introduction](#introduction)
 * [What `jigsaw` checks](#what-jigsaw-checks)
 * [What `jigsaw` does not check](#what-jigsaw-does-not-check)
 * [Configuring a host](#configuring-a-host)
 * [Planning a run](#planning-a-run)
 * [Checking plugins](#checking-plugins)
 * [Reading the report](#reading-the-report)
 * [Exit codes](#exit-codes)
 * [Running in CI](#running-in-ci)
 * [Driving it from Rust](#driving-it-from-rust)
 * [Dependencies built with erlang.mk](#dependencies-built-with-erlangmk)

 ## Introduction

Some Erlang systems are extended by Erlang/OTP applications that are built outside
the host's source tree and dropped into an installed host as plugins. Those plugins
are expected to work on a range of patch releases of the series they were built
against, even though the host's internal API is not always guaranteed to be
stable between patch releases.

## What `jigsaw` checks

`jigsaw check` reads a set of apps from directories (the plugins and the
libraries they bundle) and collects every reference they make into the
host and into Erlang/OTP. It compares each reference against the newest
patch release of one host series, the build patch, and then against every
other patch release of that series and every patch release of the
Erlang/OTP lines the host supports. A reference that the build patch
satisfies and a target does not is a finding. A missing function fails when
the code that calls it runs, not when the plugin loads.

The references that are compared:

 * remote calls, `fun m:f/N` and the MFAs passed to `rpc`, `erpc`, `meck`
   and similar
 * records, macros and types from included host headers
 * behaviours the plugins implement, with their callbacks
 * the `-spec` return shape of a called function
 * Cuttlefish partials, their keys, and validators
 * the apps the plugin set needs, which a release may not ship

Everything comes from the snapshot store. `jigsaw check` never opens a git
repository and never compiles anything.

Dependencies are checked at the exact versions each host release pins, not
over a range. A snapshot is current only if it was made with the configured
extraction settings: after changing `manifest`, `layout`, `scan_paths`,
`app_roots`, `include_apps`, `exclude_apps`, `excluded_subdirs`,
`public_modules` or `internal_modules` of a project, `jigsaw plan` lists its
snapshots as stale with the reason, and `snapshots generate --refresh-stale`
rebuilds them.

## What `jigsaw` does not check

Only production code is read: `src/`, `include/` and the Cuttlefish files
under `priv/schema/`. `test/` is never read.

A reference the build patch does not satisfy is not a finding, because it
says nothing about compatibility. It is counted, as are dynamic calls
(`apply/3`, a variable module or function), calls into Elixir modules and
references into dependencies without a `[[project]]` block. A clean report
over zero checked references means the plugin set was not read, which is why
the counts are always printed.

OTP is checked for presence only: a module, an export and an arity. Records
and macros of OTP headers are not compared, and calls inside OTP header
macros are not checked. These two limits and the pinned versions note above
are listed in every report.

A clean report is not a guarantee that a plugin works. The unchecked and
withheld sites listed in the report were not compared, and behaviour behind
an unchanged API is not compared at all.

Three facts are decided by reading source text and not by compiling, so
they can be wrong in rare cases:

 * whether a `#name{...}` is a record construction or a pattern: one that no
   rule recognises as a pattern is read as a construction, and a construction
   is what leaves fields to their defaults
 * which untracked dependency a missing module belongs to: the module is
   assumed to belong to the dependency whose name it is, or starts with, and
   an underscore
 * calls to Elixir modules: `'Elixir.Module':function(...)` is found by a
   text scan, and the module is not looked up

Checking across minor series is out of scope: a run covers one series.

## Configuring a host

A host is a `[[host]]` block next to the `[[project]]` blocks it names:

```toml
[[project]]
name = "rabbitmq-server"
git_url = "/srv/checkouts/rabbitmq-server.git"
layout = "multi_app"
app_roots = ["deps/*"]
exclude_apps = ["rabbitmq_cli"]
excluded_subdirs = ["test"]
manifest = "erlang_mk"

[[host]]
name = "rabbitmq"
project = "rabbitmq-server"
otp_project = "otp"
series_tag_pattern = "v{series}.*"
erlang_majors = [27, 28]

[host.pins]
source = "erlang_mk"
file_path = "rabbitmq-components.mk"

[host.release_roots]
apps = ["rabbit"]
makefile_variable = { file_path = "plugins.mk", variable = "PLUGINS" }

[host.erlang_support]
default = "[27.0,30.0)"

[host.erlang_support.releases]
"4.2.0" = "[26.2,29.0)"
"4.2.9" = "[27.0,29.0)"
```

 * `project` is the `[[project]]` whose tags are the host's releases
 * `otp_project` is the `[[project]]` that holds Erlang/OTP tags, and it
   must use `layout = "erlang_otp"`
 * `series_tag_pattern` has exactly one `{series}` placeholder
 * `erlang_majors` are the Erlang majors tested by default, and
   `--erlang-major` replaces them for one run
 * `[host.pins]` says where each release's dependency pins come from:
   `source = "erlang_mk"` reads `dep_*` lines from `file_path` at each tag,
   and `source = "table"` lists them per release under
   `[host.pins.releases."<version>"]`
 * `[host.release_roots]` names the apps a release is rooted at, with
   `apps`, with `makefile_variable`, or with both. The apps a release ships
   are the closure of the roots over the apps' manifests
 * `[host.erlang_support]` gives a default Erlang range and the ranges per
   release. A release uses the entry of the nearest listed release at or
   below it. `erlang_support_file_path` reads the same two tables from a
   file instead, for CI to generate

A release that does not support a requested Erlang major is a configuration
error, and not a finding: the run would otherwise report failures nobody
could deploy into.

## Planning a run

```bash
backhopper jigsaw plan --host rabbitmq --series 4.2 --plan-file-path plan.json
```

lists the patch releases of the series (pre-release tags excluded), the
Erlang range of each and the pins that changed since the previous release,
the Erlang lines to test, the snapshots that adds up to, which of them the
store lacks, and the cache key. `--up-to` names another build patch than the
newest release, and `--repo-dir-path` names a host checkout to list tags from.

`--plan-file-path` also writes the JSON envelope to a file. `jigsaw check`
and `snapshots generate --from-jigsaw-plan` read that file.

The cache key looks like `jigsaw-rabbitmq-4.2-f5-e9-3f1a9c0d27e4b6a1`: the
snapshot format, the extractor version and a digest of the required pins,
the Erlang majors and the host facts. The plan also carries the restore key,
the same string up to the digest, so CI does not spell the format out.

`snapshots generate --from-jigsaw-plan plan.json` builds the snapshots the
store lacks, are stale or were taken at another commit, and nothing else.
It reads the store when it runs, so a store restored from a cache only
gets what is still missing.

Snapshots are built several at a time, by default as many as there are
cores, up to 32. `--jobs N` sets the count. A snapshot that cannot be built is
reported in `failed` and the others are still written, and the store holds
the same files for any worker count. The 67 OTP snapshots from `OTP-27.0`
to `OTP-29.1.1` took 11.9 s on one worker and 1.0 s on 28.

## Checking plugins

```bash
backhopper jigsaw check --plan-file-path plan.json \
  --plugin-dir-path ./plugins --plugin-dir-path ./vendor
```

Each `--plugin-dir-path` is searched for app roots: a directory with a
`src/` and a manifest, which is `src/<name>.app.src` or a `Makefile` that
assigns `PROJECT`. Apps found under an app's `deps/` are bundled apps,
except those the host provides: a plugin built with erlang.mk carries the
host's own apps there, and they are left out and listed as host-provided.
An app given directly as a `--plugin-dir-path` is always checked.

Without a plan file, `--host` and `--series` (and optionally `--up-to` and
`--erlang-major`) draft the plan from the snapshot store alone. The store
must then hold the releases, and a release without a snapshot yet is not in
the run: the report says that its tags were listed from the store.

## Reading the report

The text output starts with the matrix: one row per app, and columns that
are runs of host releases with the same state for every app. A column splits
by Erlang/OTP tags only when the state of some app differs between tags.
Each cell is `clean` or `broken`.

Then come the findings. Each one names its kind (`function_missing`,
`record_fields_changed`, `partial_missing` and so on), the symbol as a
reader would search for it, the build and target sides where there are
two, the host releases or OTP tags it covers, and the files and lines of
every site. A finding never has a severity: all of them are blockers.

Warnings are listed apart from findings and never fail the run. They are an
app the plugin set bundles that the target also ships, and a record field
whose default changed.

After the findings:

 * the number of distinct references to the host and OTP that were checked
   at every target
 * `not checked`, by reason: call sites resolved inside the plugin set,
   references the build patch does not resolve, dynamic calls, calls into
   Elixir modules, references into untracked dependencies, files with an OTP
   header the reader did not see, and references used only under a version
   conditional
 * `withheld`, by reason: conditional macros, unknown specs, unreadable
   manifests, apps of untracked dependencies, and OTP apps the snapshot
   excludes
 * the host apps found in plugin `deps/` and left out
 * the limits

Three of the `withheld` reasons name what was not compared, below the
unchecked sites: conditional macros, unknown specs and OTP apps the snapshot
excludes. Each entry has the symbol, the host releases or OTP tags it was
withheld at, and the files and lines of every site that uses it. The count
of each of these reasons is the number of sites listed, as for `unchecked`.
The other two reasons are named by the warnings, and count anchors. In JSON,
`withheld` holds the entries with `reason`, `symbol`, `sites`,
`host_release_ranges` and `otp_tag_ranges`.

Two of the `not checked` reasons are also listed, site by site, below the
`withheld` section: the references the build patch does not resolve, and
dynamic calls. Each reason prints its own heading, the same phrase its count
uses, and one line per site as `app/path:line  symbol`:

```
dynamic calls:
  rabbitmq_shovel_extra/src/worker.erl:57  _:other_missing_helper/0
not resolved at v4.3.6:
  rabbitmq_shovel_extra/src/worker.erl:40  rabbit_misc:no_such_helper/1
```

A call that compiles and fails with `undef` at run time, such as
`rabbit_misc:no_such_helper(1)`, shows up here. In a dynamic call the part
the source does not spell out prints as `_`, and an arity that cannot be
read as `?`. The count of each of these two reasons is the number of lines
listed under it, so a reference used at three places counts three times.
In JSON, `unchecked` holds the same list as `reason`, `symbol`,
`app`, `path` and `line`, ordered by reason, file, line and symbol. The
other reasons are counts only. A record or macro used in a file with an
include the reader did not see is not listed, because that file's
unresolved include is the cause.

Every label is printed, including the zeros. `--formatter markdown` renders
the same content as GitHub tables and lists, and `--formatter json` renders
the envelope. In JSON, `apps[*].cells` is one cell per target in row-major
order over `host_releases` and then `otp_tags`.

A macro whose body differs between the build patch and a target is a
finding even when the difference is small, such as the year in a copyright
string. Both bodies are shown, so the difference can be read at a glance.

## Exit codes

 * `jigsaw plan` exits `0` when no snapshot is missing and `3` when some are.
   An unsupported Erlang major, an empty series and an unknown host are
   configuration errors (`78`). A release whose pins cannot be read is a
   data error (`65`)
 * `jigsaw check` exits `0` when there is no finding, with or without
   warnings, and `3` when there is a finding. A plan the store does not stock
   (missing or stale snapshots, a commit that differs) is a data error
   (`65`) that names the gaps, and `74` is a directory that cannot be read

The distinct codes let a CI step tell "the plugins are broken" (`3`) from
"the cache is cold" (`65`).

## Running in CI

Two jobs share one snapshot store through the GitHub Actions cache. GitHub
cache entries cannot be overwritten, and a pull request can only restore
entries saved on the default branch, so only the first job saves.

The warm-up job runs on the default branch and on tag pushes. It plans, restores
the cache by the plan's keys, generates what is still missing, and saves:

```yaml
name: jigsaw warm-up

on:
  push:
    branches: [main]
    tags: ["v*"]

jobs:
  warm-up:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v4
      # replace with the steps that check out the host repository with its
      # tags and the Erlang/OTP repository, where backhopper.toml says
      - name: Check out the host and OTP repositories
        run: ./ci/checkout-host-and-otp.sh

      - name: Plan
        run: |
          set +e
          backhopper jigsaw plan --host rabbitmq --series 4.2 --plan-file-path plan.json
          code=$?
          if [ "$code" -ne 0 ] && [ "$code" -ne 3 ]; then exit "$code"; fi
          echo "cache_key=$(jq -r .data.cache_key plan.json)" >> "$GITHUB_ENV"
          echo "restore_key=$(jq -r .data.restore_key plan.json)" >> "$GITHUB_ENV"

      - name: Restore snapshots
        id: restore
        uses: actions/cache/restore@v4
        with:
          path: snapshots
          key: ${{ env.cache_key }}
          restore-keys: ${{ env.restore_key }}

      - name: Generate the missing snapshots
        if: steps.restore.outputs.cache-hit != 'true'
        run: backhopper snapshots generate --from-jigsaw-plan plan.json

      - name: Save snapshots
        if: steps.restore.outputs.cache-hit != 'true'
        uses: actions/cache/save@v4
        with:
          path: snapshots
          key: ${{ env.cache_key }}

      - uses: actions/upload-artifact@v4
        with:
          name: jigsaw-plan
          path: plan.json
```

A new patch release restores the previous cache through the restore key and
generates only its own snapshots. An extractor or snapshot format change
starts from nothing.

The check job runs on pull requests and needs only the plugin directories
and the store. It does not check the host out, so it takes `plan.json` from
the warm-up job's latest run on the default branch:

```yaml
name: jigsaw check

on: pull_request

permissions:
  actions: read
  contents: read

jobs:
  check:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v4
      # replace with the step that builds or checks out the plugin set
      - name: Build the plugins
        run: ./ci/build-plugins.sh

      - name: Download the plan
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          run_id=$(gh run list --workflow "jigsaw warm-up" --branch main \
            --status success --limit 1 --json databaseId --jq '.[0].databaseId')
          gh run download "$run_id" --name jigsaw-plan
          echo "cache_key=$(jq -r .data.cache_key plan.json)" >> "$GITHUB_ENV"
          echo "restore_key=$(jq -r .data.restore_key plan.json)" >> "$GITHUB_ENV"

      - name: Restore snapshots
        id: restore
        uses: actions/cache/restore@v4
        with:
          path: snapshots
          key: ${{ env.cache_key }}
          restore-keys: ${{ env.restore_key }}

      # a cold cache falls back to generating the snapshots, which takes
      # minutes and is never saved, so the default branch stays the only writer
      - name: Check out the host and OTP repositories
        if: steps.restore.outputs.cache-hit != 'true'
        run: ./ci/checkout-host-and-otp.sh

      - name: Generate the missing snapshots
        if: steps.restore.outputs.cache-hit != 'true'
        run: backhopper snapshots generate --from-jigsaw-plan plan.json

      - name: Check
        run: |
          set +e
          backhopper --formatter markdown jigsaw check \
            --plan-file-path plan.json --plugin-dir-path ./plugins > summary.md
          code=$?
          cat summary.md >> "$GITHUB_STEP_SUMMARY"
          exit "$code"
```

On a cold cache `jigsaw check` exits with `65` and names the missing
snapshots. That is a correct answer and not a useful one, which is why the
example generates them when the exact key was not restored. The time budget
of a few minutes applies to the warm case, which is every pull request after
the first warm-up on the default branch.

## Driving it from Rust

`backhopper-driver` has a `jigsaw()` group with typed builders:

```rust
let plan = driver
    .jigsaw()
    .plan()
    .host(HostName::new("rabbitmq")?)
    .series("4.2".parse()?)
    .plan_file_path("plan.json")
    .run()?;

let report = driver
    .jigsaw()
    .check()
    .plan_file("plan.json")
    .plugin_dir("./plugins")
    .run()?;
```

`run` on a plan builder without both a host and a series does not compile,
and neither does `run` on a check builder without a plan source and at
least one plugin directory. A report with findings and a plan with missing
snapshots exit with `3` and are returned as results. A plan the store does
not stock exits with `65` and is a `DriverError::ToolError`.

## Dependencies built with erlang.mk

A dependency's `[[project]]` reads its app manifest from `src/<app>.app.src`
unless it sets `manifest = "erlang_mk"`. `cowboy`, `cowlib`, `ranch` and
`gun` have no `.app.src`, so without that setting the report warns
`manifest_unreadable` for them and withholds every comparison that needs
the closure of shipped apps. Changing `manifest` does not invalidate
stored snapshots: delete the dependency's snapshots and run
`snapshots generate --from-jigsaw-plan` again.
