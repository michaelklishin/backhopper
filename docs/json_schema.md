# JSON Output Schema

Every `backhopper` command supports `--formatter json`. The output is a
single JSON object with a stable envelope:

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

## Common payload types

### `data` for `check {patch, commit, range}`

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

### `data` for `version`

```json
{
  "name": "backhopper",
  "version": "0.4.0"
}
```

### `data` for `series list`, `series show`, `projects list`, `projects show`

Self-documenting: each carries a `name` (or `project`) field plus
configuration-derived data. Refer to the Rust payload structs.

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

## Drift-guarded fixtures

The unit-test suite under
`crates/backhopper-cli/tests/unit/json_envelope_unit_tests.rs`
serializes representative payloads against checked-in JSON fixtures
under `crates/backhopper-cli/tests/fixtures/json/`. Any divergence
between the Rust types and these fixtures fails CI; the fixtures must
be regenerated and reviewed when intentional changes ship.
