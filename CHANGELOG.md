# Changelog

## v0.37.0 (in development)

### Enhancements

 * New `jigsaw` command group: checks Erlang apps that are hosted as
   plugins (RabbitMQ plugins, for example) against every patch release of
   one host series and of the Erlang/OTP lines it supports
 * `jigsaw plan` lists the releases, the snapshots they need and which
   of them the store lacks
 * `jigsaw check` compares what the plugin directories reference against
   the snapshots `jigsaw plan` lists. It reads the snapshot store only and
   never opens a git repository
 * `snapshots generate --from-jigsaw-plan` builds the snapshots a `jigsaw`
   plan lists as missing or stale
 * A new `[[host]]` block in `backhopper.toml` describes the `jigsaw` host,
   and `[[project]]` gains `manifest`
 * See `docs/jigsaw.md` for how `jigsaw plan` and `jigsaw check` work
 * A snapshot records the extraction settings it was made with. A snapshot
   made with other settings is stale: `doctor` shows it as `OUTDATED`, and
   `snapshots generate --refresh-stale` rebuilds it
 * `snapshots generate` builds snapshots on several threads: `--jobs N`,
   by default the number of cores, up to 32
 * Assorted domain types improvements, see `docs/types.md`
 * `snapshots generate --refresh-stale` rebuilds a snapshot that is on
   disk but stale or has no extractor version
 * `doctor` and `snapshots verify --all` now treat a snapshot with no
   `extractor-version` header (written before `0.31.0`) as stale rather
   than passing. `doctor` reports it as `Unversioned`, and `verify --all`
   counts such occurrences separately
 * `backhopper-driver` gains `jigsaw().plan()` and `jigsaw().check()`
   builders
 * New crate `backhopper-jigsaw`

### Breaking Changes

 * Snapshots written by extractor version 8 or earlier are `Stale`, and
   `snapshots generate --refresh-stale` rebuilds them
 * The envelope schema version is now 18, which adds the `jigsaw plan` and
   `jigsaw check` payloads. Version 17 is frozen
 * The `erlang_otp` layout defaults `tag_prefix` to `OTP-`, not `v`
 * `parse_components_mk`, `parse_pin_line`, `DepPin` and `DepSource` moved
   from the CLI to `backhopper_core::erlang_mk`. A `dep_` line with a name
   that `DependencyName` refuses is no longer a pin, and `series sync`
   lists it as skipped
 * `SnapshotHeader`, `HrlFile`, `RecordField` and `Snapshot` gain fields,
   and the extractors' `extract_source` take `ScannedFile`s

### Dependency Upgrades

 * `gix` was upgraded from `0.86.0` to `0.89.0`
 * `bel7-cli` was upgraded from `0.14.0` to `0.15.0`
 * `tabled` was upgraded from `0.21.0` to `0.22.0`
 * The minimum Rust version is now `1.98.1`

### Bug Fixes

 * The call reader no longer counts the statement commas in the body of a
   `fun`, `case`, `try`, `begin`, `receive`, `if` or `maybe` as arguments
   of the enclosing call, so `lists:foreach(fun(X) -> A, B end, L)` is
   `lists:foreach/2` and not `lists:foreach/3`
 * The call reader no longer reads a catch pattern such as
   `error:badarg ->` as a call
 * A snapshot written before extractor versioning existed had no
   `extractor-version` header, and the staleness check read that
   empty value as current instead of as the most suspect file in the
   store. This let a stale pre-`0.31.0` snapshot pass `doctor` and
   produce an incorrect `Incompatible` verdict on a correct pick
 * `Module.clause_heads` is a `BTreeMap<FunArity, ...>`; serialising a
   module with a populated map failed with `key must be a string`,
   because `serde_json` refuses a non-string map key. The map now
   serialises its key through `FunArity`'s `Display`
 * `config show --formatter json` spelled a self-project's `kind` as
   `self_repo`; every other surface already said `self`. It now says
   `self` everywhere
 * A record field's `::` type annotation (`vhost :: rabbit_types:vhost()`)
   is now read as a type reference and not as a call. Field defaults
   are still scanned for calls
 * An `{M, F, Args}` tuple in a registration attribute
   (`-rabbit_boot_step`'s `mfa` field and similar) is now read as a
   function reference and resolved against the target tree, the same
   as a `mod:fun(...)` call. Previously such a tuple was never
   extracted at all, so a callee it named could be undefined on the
   target with no finding
 * A patch that adds `-compile(export_all)` to a module without also
   touching its `-export` list in the same diff no longer flags calls
   into that module as undefined on target: the export surface may
   cover the callee in a way the diff itself does not show, so the
   axis withholds instead of guessing from the small explicit list
 * A module is now hidden only by a module-level marker:
   `-moduledoc false.` (or `hidden`), or an edoc `@hidden` tag before
   `-module`. A `-doc hidden.` or `-doc false.` on one type or function,
   or an `@hidden` above one function, used to hide the whole module by
   mistake, which particularly badly affected Erlang/OTP modules
 * The export, `-export_type`, `-import` and `-optional_callbacks`
   lists are now read with their comments stripped off
 * With a commit to read (`check commit`, `check merge` and
   `check batch`), the target-tree axes now classify an added line
   against the whole file the commit leaves, not only one specific hunk

### Removals

 * `backhopper_erlang::visibility::detect_visibility_hints` and
   `VisibilityHints` are removed
 * `TargetWalkIndex.truncated` is removed; it had no reader
 * `WireConstantDecl::new` is removed; the family defaults are built
   from one `const` table per family, and the struct literal is the
   door for a test

## v0.32.0 (August 13, 2026)

### Bug Fixes

 * The macro, record, and exported-type axes now read `-include` and
   `-include_lib` directives from the patch's added text, so a file a
   patch adds no longer reports every header-supplied macro and record
   as undefined on the target. The added module and its header are analyzed
   together

### Removals

 * The `siblings doctor` verb is removed along with its scoring
   pipeline, its per-family drift vocabulary, and its workspace cache.
   It has never proven its work in real world use
 * As a consequence the envelope schema version moves to 16, which
   drops the `siblings_doctor_payload` document, the `siblings` cache
   level, and the `siblings` counters in the `cache stats` payload

## v0.31.0 (August 5, 2026)

### Enhancements

 * Erlang lexers now handle EEP-66 sigil strings (`~"..."`, `~S"""`,
   plus the delimiter forms)
 * Support for `-nominal` types (EEP 69, new in Erlang 28)
 * The `-spec` type parser understands ranges (`1..255`), binary
   specs (`<<_:8>>`), annotated types (`Name :: t()`), based and
   underscored integers, char literals, and the Erlang 27 and 28
   built-ins (`dynamic()` and the `nonempty_*` family)

### Bug Fixes

 * A based numeric literal (`16#ff`, `16#fe.fe#e16`) no longer
   produces a phantom record reference
 * A dotted expression or based float in a `-define` body no longer
   truncates the macro table entry
 * A triple-quoted string in a function body no longer swallows the
   clause heads after it
 * Record fields with map defaults or map types
   (`f = #{a => b} :: map()`) no longer truncate at the `=>`
 * A form ending in `.` immediately followed by a `%` comment now
   terminates where the compiler says it does
 * Comments inside attribute bodies no longer perturb the argument
   scanners: a comment with a stray apostrophe in `khepri.erl`'s
   export list was silently dropping 93 exports per tag, and entries
   directly below a comment line were lost in `ra_lib`


## v0.30.0 (August 1, 2026)

### Enhancements

 * Erlang lexers now handle EEP-64 triple-quoted strings

### Bug Fixes

 * Avoid false positives on patches that modify a function `-spec` and its arity
 * Fix phantom arity readings with functions such as `lists:foreach/4` that
   take an anonymous function that can be multi-line


## v0.28.0 (July 18, 2026)

### Bug Fixes

 * Qualified-call resolution against a target repo no longer produces
   false `qualified_call_undefined_on_target` findings for certain hunks

### Release Infrastructure

 * Adopt `michaelklishin/rust-release-action` for releases

 
## v0.27.0 (July 17, 2026)

### Enhancements

 * Major internal refactoring
 * `backhopper` now can detect `-export_type` entries a patch that
   export a type that does not exist on the target branch
 * `siblings doctor` no longer renders a run that analysed nothing the
   same as a run that found nothing: the text formatter now helps
   tell one case from the other
 * `m:f/a` references in Elixir parts of the diff
   are now analyzed for `rpc` use and resolved against the Erlang target tree and
   can be caught at `check` time
 * Symbol findings on the target are no longer dropped when every
   verdict is inapplicable: each row checked against a target carries a
   `target_findings` list beside the pin verdicts, and the clearance,
   summary, and exit code reflect it. Before, a suite-only pick could
   reference a function the target never defines and still exit 0
 * OTP modules such as `rpc` are no longer reported as missing by
   `--resolve-untracked-modules`
 * Apply conflict forecast accuracy can now be measured:
   `evaluate_forecasts` compares stored forecasts with how picks
   actually applied and lists the conflicts the forecast missed
 * Predicted apply conflicts are no longer dropped when every verdict
   is inapplicable: each row checked against a target carries a
   per-path `apply` forecast, and the clearance and exit code reflect it
 * `-spec` return type drift between the checkouts is now detected for
   both qualified and unqualified calls, reported as non-blocking
   reasons
 * `m:f/a` references passed to `meck:expect`, `rpc:call`, `erpc:call`,
   and CT broker rpc helpers are now resolved against the target tree
 * Calls whose argument lists wrap across lines are now checked at
   their exact arity; before, they were skipped
 * A used macro whose `-define` value differs between the checkouts is
   now reported as non-blocking `macro_value_drift`
 * First-party behaviours are now compared callback by callback between
   the checkouts
 * New `tree show <module>` verb: a module's API surface from any repo
   ref, in the snapshot format
 * New `check cascade` verb: one commit set against several series in
   one run, each series using its own target checkout, which
   `[[series]]` stanzas can now name in `backhopper.toml`
 * Faster SHA prefix resolution and commit lookups on large repositories
 * Suite selection: one changed module pulling in an outsized share of
   suites is now reported as a single `broad_impact` row instead of a
   near-full suite list
 * Batches where every candidate is inapplicable now report a distinct
   `ZeroDomain` clearance instead of `Clean`
 * `doctor` now flags snapshots generated by an older extractor version
   as `STALE` and exits non-zero

### Bug Fixes

 * Multi-line patch numbers could drift in some scenarios
 * Import tracking: a call resolved through an `-import` the patch itself adds is no
   longer flagged as undefined on the target branch.
 * Reported line numbers no longer shift on generated commit diffs:
   backhopper now assembles hunk headers itself instead of trusting the
   imara-diff printer
 * A bitstring type with an internal comma (e.g. `<<_:8, _:_*8>>`) no
   longer splits a spec argument or record field in two. The extractor
   version is now 5, so `snapshots verify --all` flags snapshots
   generated before the fix

## v0.19.0

### Enhancements

 * A path absent on the target while others are present is now a
   non-blocking `target_path_absent` reason, and no longer silently dropped
 * A qualified `m:f/a` call a patch adds into a first-party module the
   target branch does not export is now a non-blocking
   `qualified_call_undefined_on_target` reason; before, only unqualified
   calls were checked against the target tree

### Bug Fixes

 * `?MACRO` and `#record` uses are no longer reported as undefined when
   the file includes an unreadable stdlib header (`eunit`, `logger.hrl`):
   the define set is incomplete, so the check skips them
 * Variable applications (`Fun(...)`), attribute forms (`-export(...)`),
   and `-spec` type names are no longer read as undefined local calls;
   the scanner skips them instead of re-lexing their tails
 * `macro_undefined_on_target`, `record_undefined_on_target`, and
   `local_call_undefined_on_target` now report the line in the file, not
   the offset within the patch's added lines
 * The `check` text table names the target-tree reasons (macro, record,
   local-call, and qualified-call undefined-on-target) instead of
   showing `UnknownReason`
 * A call or clause head whose argument list wraps across lines now
   resolves at exact arity. The per-line scanner lost the arity and read
   the call as any-arity, so a wrapped call to a missing arity was
   accepted when the function existed at another arity

### Removed

 * `--summary-only` is removed; use `--formatter summary` (JSONL) or
   `--formatter text-summary` (text)

## v0.18.0

### Enhancements

 * Expanded `check batch` text output
 * `check` JSON envelopes gained a `self_projects` field; envelope
   schema version is now 12
 * `check` JSON envelopes now carry `resolver_coverage` and
   and `fingerprint_version`
 * `backhopper-core` and `backhopper-driver` expose the verdict-to-
   measurement mappings to the caller

## v0.16.0

Initial release.
