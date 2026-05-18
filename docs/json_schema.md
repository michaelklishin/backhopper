# JSON Output Schema

Every `backhopper` command supports `--formatter json` except
`shell completions`, which emits raw shell-completion scripts. The
JSON output is a single object with a stable envelope:

```json
{
  "schema_version": 1,
  "command": "check patch",
  "data": { ... },
  "exit_code": 0
}
```

## Envelope fields

* `schema_version`: integer. Currently `1`. Bumped only when the
  envelope structure changes in a breaking way. Consumers should check
  this and refuse to parse unknown versions
* `command`: short string identifier for the command that produced the
  output: e.g. `"check patch"`, `"snapshots list"`, `"version"`. Stable
  across releases unless a command is renamed
* `data`: the command-specific payload. Its shape varies; see the
  per-command sections below
* `exit_code`: integer. Always present. `0` on success;
  non-zero on a non-`Compatible` verdict or other application-level
  failure. Mirrors the process exit code

## `schema_version` policy

`schema_version` is bumped when:

* a field in the envelope itself is added, removed, or renamed
* the semantics of a field change in a way that would silently
  mis-parse with an old consumer

Field additions to `data` (the command-specific payload) do *not* bump
`schema_version`. Consumers must tolerate unknown fields under `data`
gracefully (treat as `non_exhaustive`).

## Source of truth

The Rust types under `crates/backhopper-cli/src/commands/*.rs` (and
`crates/backhopper-core/src/model/*.rs` for verdicts and diagnostics)
are the authoritative schema. `cargo doc --no-deps` renders them with
field-level documentation. The examples below are normative for the
envelope and informative for `data`.

## Flag interactions

A few flags add fields or reasons to `data`:

* `--source-tag <T>` or `--source-series <name>` on `check`: enables
  source-pin diffing. Verdicts can then carry `signature_changed` and
  `record_fields_changed` reasons against the source snapshot
* `--resolve-untracked-modules --repo-dir-path <p>` on `check`:
  cross-references each untracked-call module name against the
  target-pin checkout. Modules missing from the checkout produce
  `untracked_module_missing` reasons that flip the verdict to
  `incompatible`
* `--summary-only` on `check {patch, commit, range, batch}`: affects
  the text formatter only. The JSON payload is unchanged

## `check` payloads

### `data` for `check patch`, `check commit`, `check range`

`check commit` and `check range` both build a unified diff and route
through the same handler as `check patch`. The envelope's `command`
field reads `"check patch"` for all three.

```json
{
  "queried_against": {
    "kind": "pin",
    "project": "ra",
    "tag": "v2.0.0"
  },
  "results": {
    "results": [
      {
        "pin": { "project": "ra", "tag": "v2.0.0" },
        "verdict": {
          "verdict": "compatible"
        },
        "tracked_refs": 3
      }
    ],
    "summary": {
      "compatible": 1,
      "requires_adaptation": 0,
      "incompatible": 0
    }
  },
  "diagnostics": {}
}
```

`queried_against` is either:

```json
{ "kind": "pin", "project": "P", "tag": "T" }
```

or:

```json
{
  "kind": "series",
  "name": "stable-3.x",
  "pins": [
    { "project": "ra", "tag": "v2.0.0" },
    { "project": "khepri", "tag": "v0.18.0" }
  ]
}
```

`results.results[].verdict.verdict` is one of `"compatible"`,
`"requires_adaptation"`, or `"incompatible"`. When non-`Compatible`,
`reasons` is present:

```json
{
  "verdict": "incompatible",
  "reasons": [
    {
      "kind": "missing_symbol",
      "symbol": { ... },
      "first_seen_at_tag": null,
      "suggested_replacement": null
    }
  ]
}
```

### `Reason` variants

Tagged with `"kind"` (snake_case):

| `kind` | shape |
|---|---|
| `missing_symbol` | `{ symbol, first_seen_at_tag, suggested_replacement }` |
| `arity_changed` | `{ module, function, expected, found }` |
| `signature_changed` | `{ module, function, arity, expected_spec, found_spec }` |
| `file_absent` | `{ path }` |
| `context_drift` | `{ path, hunk_index }` |
| `deprecated_usage` | `{ symbol, since, replacement }` |
| `now_hidden` | `{ module }` |
| `record_fields_changed` | `{ record, expected, found }` |
| `unsupported_file_type` | `{ path }` |
| `untracked_module_missing` | `{ module }` |
| `clause_mismatch` | `{ module, function, arity, call_args, pin_clauses }` |

### `ArgShape`

Used inside `clause_mismatch`. Tagged with `"kind"`:

| `kind` | additional fields |
|---|---|
| `variable` | – |
| `atom` | `name` (string) |
| `integer`, `float`, `binary`, `list`, `string`, `fun` | – |
| `tuple` | `size` (integer) |
| `record` | `name` (record name) |
| `unknown` | – |

`unknown` and `variable` on either side accept anything: the analyzer
only flags a `clause_mismatch` when both sides are concrete enough to
prove a position-by-position mismatch.

### `Diagnostics`

```json
{
  "untracked_calls": { "lists": 2, "io": 1 },
  "untracked_records": {},
  "unanalyzed": { "apply": 0, "variable_dispatch": 0 }
}
```

All three fields are omitted when empty (`is_empty` skip).

### `data` for `check batch`

```json
{
  "queried_against": [
    {
      "series": "stable-3.x",
      "pins": [{ "project": "ra", "tag": "v2.0.0" }]
    }
  ],
  "results": [
    {
      "commit": "1a2b3c4d",
      "series": "stable-3.x",
      "verdict": { ... },
      "diagnostics": {}
    }
  ]
}
```

## `snapshots` payloads

### `data` for `snapshots list`

```json
{
  "project": "ra",
  "tags": ["v1.0.0", "v2.0.0"]
}
```

### `data` for `snapshots list_tags`

Array of per-project entries:

```json
[
  {
    "project": "ra",
    "tags_without_snapshots": ["v3.0.0-rc1"]
  }
]
```

### `data` for `snapshots generate`

Array of per-project entries:

```json
[
  {
    "project": "ra",
    "captured": 2,
    "skipped": 5,
    "failed": [],
    "tags": ["v2.0.0", "v2.0.1"],
    "ignored_non_tag_refs": []
  }
]
```

### `data` for `snapshots lookup`

```json
{
  "project": "ra",
  "tag": "v2.0.0",
  "results": [
    {
      "mfa": "ra:start/0",
      "found": true,
      "visibility": "public"
    }
  ]
}
```

### `data` for `snapshots show`

The full `Snapshot<Canonical>` value, including `header`, `modules`,
and `headers`. Refer to `backhopper_core::model::snapshot::Snapshot`
for the field-by-field schema. The text formatter writes the canonical
on-disk form; the JSON formatter serializes the same data via
`serde`.

### `data` for `snapshots verify`

```json
{
  "project": "ra",
  "tag":     "v2.0.0",
  "matches": true
}
```

`matches` is `false` (and `exit_code` is `1`) when the on-disk
snapshot differs from a fresh extraction of the tag.

### `data` for `snapshots rebuild`

```json
{
  "project": "ra",
  "tag":     "v2.0.0",
  "rebuilt": true
}
```

`rebuilt` is `false` when `--dry-run` is set.

### `data` for `snapshots modules`

```json
{
  "project": "ra",
  "tag":     "v2.0.0",
  "modules": [
    { "name": "ra", "visibility": "public", "exports": 12, "callbacks": 0 }
  ],
  "headers": ["include/ra.hrl"]
}
```

### `data` for `snapshots exports`

```json
{
  "project": "ra",
  "tag":     "v2.0.0",
  "module":  "ra",
  "exports": ["start/0", "start/1", "stop/0"]
}
```

`exit_code` is `1` when the module isn't present in the snapshot.

### `data` for `snapshots diff`

```json
{
  "project": "ra",
  "from":    "v2.0.0",
  "to":      "v2.1.0",
  "modules_added":   ["ra_new_mod"],
  "modules_removed": ["ra_old_mod"],
  "exports_added":   [{ "module": "ra", "fun_arity": "fresh/1" }],
  "exports_removed": [{ "module": "ra", "fun_arity": "gone/0" }]
}
```

## `config` payloads

### `data` for `config path`

```json
{ "path": "/Users/me/work/backhopper.toml" }
```

### `data` for `config show`

The full parsed configuration: defaults, projects, series. Refer to
`backhopper_core::config::Config` for the field-by-field schema.

### `data` for `config validate`

```json
{ "ok": true }
```

`exit_code` is `0` on a valid file, non-zero on a parse or validation
error (in which case the error is printed to stderr instead).

## `xref` payloads

Every `xref` subcommand returns an `AnalysisResult` value. The JSON
shape follows the underlying struct one-to-one. All share the same
top-level convention: a single field carrying the entries
(`entries`, or a domain-named alternative).

### `data` for `xref list_callers` and `xref list_callees`

```json
{
  "target": { "module": "rabbit_db", "function": "set", "arity": 2 },
  "entries": [
    {
      "caller": { "module": "rabbit_vhost", "function": "save", "arity": 1 },
      "kind":   "direct",
      "locations": [{ "path_id": 7, "start": {"line": 42, "column": 5, "byte_offset": 1234} }]
    }
  ]
}
```

`list_callees` swaps `caller` for `callee` (a `FunctionRef`, which can
be `concrete`, `unresolved_module`, `unresolved_function`, or
`unresolved_both`). `--transitive` walks the relation's closure.

### `data` for `xref list_undefined`, `list_unused_exports`, `list_unused_locals`, `list_deprecated_calls`, `list_unresolved`

A single object with an `entries` array of the matching call sites or
function references. The shape of each entry depends on the analysis:

* `list_undefined`: `[{ caller, callee_module, callee_function, callee_arity, location }]`
* `list_unused_exports`: `[{ mfa, def_loc }]`
* `list_unused_locals`: `[{ mfa, def_loc }]`
* `list_deprecated_calls`: `[{ caller, target, tier, location }]`
* `list_unresolved`: `[UnresolvedCallSite]` (see the `CallKind` and
  `FunctionRef` types in `backhopper-xref-graph`)

### `data` for `xref list_module_deps`

```json
{
  "source":  "rabbit_db",
  "entries": ["rabbit_misc", "rabbit_log"]
}
```

`--forward` reports the modules that `source` calls; the default
reports modules that call `source`.

### `data` for `xref list_behaviour_users`

```json
{
  "behaviour": "gen_server",
  "entries":   ["rabbit_amqqueue", "rabbit_channel"]
}
```

### `data` for `xref list_module_cycles`

```json
{
  "cycles": [
    ["mod_a", "mod_b", "mod_a"]
  ]
}
```

## `suites` payloads

### `data` for `suites list_for_modules`, `suites list_for_mfas`

```json
[
  { "application": "rabbit", "module": "vhost_SUITE" },
  { "application": "rabbit", "module": "metadata_store_phase1_SUITE" }
]
```

Each entry's `application` is `null` when the suite couldn't be
mapped to an in-tree application.

### `data` for `suites list_callers_of`

The same `CallersOf` shape as `xref list_callers`.

### `data` for `suites plan`

```json
{
  "entries": [
    {
      "suite": {
        "application": "rabbit",
        "module": "vhost_SUITE",
        "path": "deps/rabbit/test/vhost_SUITE.erl"
      },
      "reasons": [
        { "kind": "same_app_caller", "application": "rabbit", "module": "rabbit_db" }
      ]
    }
  ]
}
```

Entries are sorted by `suite`. Each entry carries one or more
`SuiteInclusionReason` values, tagged with `"kind"`:

| `kind` | additional fields |
|---|---|
| `test_modified` | `path` |
| `unit_or_prop_sweep` | `application`, `triggering_modules` |
| `same_app_caller` | `application`, `module` |
| `cross_app_caller` | `library_application`, `module` |
| `configured_rule` | `rule_name`, `triggering_path` |

## `rabbitmq` payloads

### `data` for `rabbitmq infer_series`

```json
{
  "series": [
    {
      "name": "rabbitmq-4.1",
      "pins": [
        { "project": "ra", "tag": "v2.16.13" },
        { "project": "khepri", "tag": "v0.16.0" }
      ]
    }
  ]
}
```

Inferred from a RabbitMQ checkout's `rabbitmq-components.mk` plus the
project list in the workspace config.

## `projects` and `series` payloads

### `data` for `projects list`

Array of project descriptors:

```json
[
  {
    "name": "ra",
    "git_url": "/path/to/ra.git",
    "language": "erlang",
    "tag_prefix": "v"
  }
]
```

### `data` for `projects show`

A single project descriptor (same shape as one entry above), plus the
configured `public_modules`, `internal_modules`, and `scan_paths`
lists.

### `data` for `series list`, `series show`

Array of (or single) series descriptors:

```json
{
  "name": "rabbitmq-4.1",
  "pins": [
    { "project": "ra", "tag": "v2.16.13" }
  ]
}
```

## `version`

```json
{
  "name": "backhopper",
  "version": "0.4.0"
}
```

## Drift-guarded fixtures

The unit-test suite under
`crates/backhopper-cli/tests/unit/json_envelope_unit_tests.rs`
serializes representative `Verdict` and `Diagnostics` payloads against
checked-in JSON fixtures under
`crates/backhopper-cli/tests/fixtures/json/`. Coverage today:
`verdict_compatible`, `verdict_with_reasons`, `verdict_with_clause_mismatch`,
`verdict_with_untracked_module_missing`, `verdict_with_unsupported_file_type`,
`diagnostics_populated`. The drift guard locks the `Verdict` and
`Diagnostics` types and the `Reason` and `ArgShape` enums; it does
not lock the full per-command envelope. Any divergence between the
Rust types and these fixtures fails CI; the fixtures must be
regenerated and reviewed when intentional changes ship.
