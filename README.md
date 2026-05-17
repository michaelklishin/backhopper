# backhopper

`backhopper` is a tool that simplifies dependency compatibility analysis
of git commits (patches) for Erlang- and Elixir-based tools. 

Given a set of public API snapshots of dependencies, it can detect whether a patch uses any API
elements that are not available in a specific dependency version.


## What Problem It Solves

When a pull request or a commit (patch) is backported, it has to be
verified against the public API of an older set of dependencies,
ideally before the backporting is done.

Tools such as `xref` require backporting before such verification
can happen.

`backhopper` turns the tables, allowing the essential part of such API
compatibility verification to happen before backporting. It records the
public API of each dependency at every git tag, then checks any patch,
commit, range, or merge commit against the dependency versions a target
branch uses.


## Project Maturity

Young project, breaking changes are likely.


## Binary Releases

Binary releases are available [on the Releases page](https://github.com/michaelklishin/backhopper/releases).


## Usage

### Getting Help

```shell
backhopper help
```

All command groups and individual commands support `--help`:

```shell
backhopper check --help
backhopper check commit --help

backhopper snapshots --help
backhopper snapshots lookup --help
```

Prefixes are inferred when unambiguous, so `backhopper sn li` is the same as
`backhopper snapshots list`.


### Configuration

`backhopper` reads a TOML config file. By default it looks for
`backhopper.toml` in the current directory; override the path with `-c`
or the `BACKHOPPER_CONFIG_FILE_PATH` environment variable.

Each dependency is a "project". Each release line is a named "series"
that pins one tag per project. Example:

```toml
config_version = 1

[defaults]
snapshot_dir = "/path/to/snapshots"

[[project]]
name    = "lib_a"
git_url = "/path/to/lib_a.git"

[[project]]
name    = "lib_b"
git_url = "/path/to/lib_b.git"

[[series]]
name = "stable-3.x"
pins = [
    { project = "lib_a", tag = "v2.0.4" },
    { project = "lib_b", tag = "v1.8.0" },
]
```


### Capturing Snapshots

`snapshots generate` walks every git tag of a project, parses the public
API, and writes one text file per tag:

```shell
backhopper snapshots generate --project lib_a
```

Re-running is cheap; only new tags are scanned. To preview what would be
captured without writing anything, add `--dry-run`. To list tags that have
no snapshot on disk yet without touching the store:

```shell
backhopper snapshots list_tags --project lib_a
```


### Looking Up the API at a Tag

For ad-hoc questions ("was this function exported at v1.8.0?"):

```shell
backhopper snapshots lookup --project lib_a --tag v2.0.4 \
                            --mfa some_module:some_function/2

backhopper snapshots modules --project lib_a --tag v2.0.4

backhopper snapshots diff --project lib_a --from v2.0.3 --to v2.0.4
```


### Checking a Patch Against a Series

This is the main thing the tool does. Given a commit on a newer branch,
check whether it will still work against a series that pins older
dependency versions:

```shell
backhopper check commit --series stable-3.x \
                        --repo-dir-path /path/to/your_repo.git \
                        1a2b3c4d
```

Short SHAs, tags, or anything `git rev-parse` understands are accepted.

For a commit range, or for a merge commit (which expands to the diff
between the merge's first parent and the merge itself):

```shell
backhopper check range --series stable-3.x \
                       --repo-dir-path /path/to/your_repo.git \
                       --range v3.0.0..HEAD

backhopper check range --series stable-3.x \
                       --repo-dir-path /path/to/your_repo.git \
                       --merge-commit 9f8e7d6c
```

A raw unified diff also works, either piped in or read from a file:

```shell
git format-patch -1 --stdout HEAD | \
  backhopper check patch --series stable-3.x

backhopper check patch --series stable-3.x /path/to/the.patch
```

For many commits at once, `check batch` evaluates each commit against one
or more series in a single invocation:

```shell
backhopper check batch \
    --series stable-3.x,stable-4.x \
    --repo-dir-path /path/to/your_repo.git \
    --commits-file-path candidates.txt
```

A clean run prints one row per pinned dependency: the verdict and how
many symbols were checked.

```
compatible: 2, requires_adaptation: 0, incompatible: 0

┌──────────────────┬────────────┬────────┬──────────────────────────────┐
│ pin              │ verdict    │ reason │ detail                       │
├──────────────────┼────────────┼────────┼──────────────────────────────┤
│ lib_a@v2.0.4     │ Compatible │ -      │ 3 tracked symbols referenced │
│ lib_b@v1.8.0     │ Compatible │ -      │ 0 tracked symbols referenced │
└──────────────────┴────────────┴────────┴──────────────────────────────┘
```

`0 tracked symbols referenced` means the patch never touched that
project's API surface, so the verdict is trivially safe. A non-zero
count is the trust signal: `backhopper` actually checked that many
call sites against the pinned tag.


### Seeing What Was Skipped

The verdict only covers tracked projects. Calls to anything else — the
OTP standard library, third-party libraries you did not configure, or
dynamic dispatch through variables — are deliberately left out so they
do not drown the signal. Add `--show-untracked-calls` to see what was
skipped:

```shell
backhopper check commit --series stable-3.x \
                        --repo-dir-path /path/to/your_repo.git \
                        --show-untracked-calls 1a2b3c4d
```

The footer groups skipped items into three categories. All three are
informational only and never change the verdict:

* **Untracked module calls** are calls to modules that no tracked project
  owns. The output suggests a guess at a project name (a module called
  `something_helper` is reported as "project: something?") so you can
  tell whether you forgot to track a dependency or whether the call is
  genuinely outside the tool's scope
* **Untracked records** are `#name{...}` references for records that no
  tracked snapshot declares. Same idea as untracked calls but for the
  record namespace
* **Unanalyzed dynamic dispatch** is `apply/3`, the `spawn` family, and
  `Mod:fun(...)` where the module or function name is a variable. These
  cannot be resolved statically. `backhopper` counts them so they are
  visible rather than silently dropped

Calls to the OTP standard library (`lists`, `gen_server`, `application`,
and so on) are suppressed by default even when `--show-untracked-calls`
is on, because they rarely change the conclusion. Add `--show-otp-calls`
to include them.


### JSON Output

Every command accepts `--formatter json`:

```shell
backhopper --formatter json check commit \
    --series stable-3.x \
    --repo-dir-path /path/to/your_repo.git \
    1a2b3c4d
```

Diagnostics live under `data.diagnostics`, kept separate from
`data.results` so a JSON consumer cannot mistake them for actionable
verdicts.


## Subprojects

 * `crates/backhopper-core` is the library: model types, snapshot I/O,
   store, `gix`-based git access, and the compatibility analyzer
 * `crates/backhopper-erlang` is the Erlang source surface extractor
 * `crates/backhopper-elixir` is the Elixir source surface extractor
 * `crates/backhopper-xref-graph` provides the call-graph primitives
   (vertices, relations, set algebra, transitive closure)
 * `crates/backhopper-xref-reader` turns Erlang source into call-graph
   input
 * `crates/backhopper-xref` is the cross-reference façade and the
   test-suite selection adapter
 * `crates/backhopper-cli` is the `backhopper` command line tool


## License

This project is double licensed under the MIT License and the Apache License, Version 2.0.

See `LICENSE-APACHE` and `LICENSE-MIT` for details.

SPDX-License-Identifier: Apache-2.0 OR MIT


## Copyright

(c) 2026 Michael S. Klishin and Contributors.
