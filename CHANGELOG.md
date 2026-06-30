# Changelog

## v0.22.0 (in development)

### Enhancements

 * Ambiguous SHA-prefix resolution disambiguates through gix's indexed
   `lookup_prefix` (a binary search over the pack and loose object
   indices) instead of scanning the whole object database
 * `GitRepo::has_commit` checks the object header instead of decoding
   the full commit
 * Suite selection now caps single-module fanout: when one changed module
   reaches an outsized share of discovered suites (at least eight, and
   more than a third), the suites it alone pulled in are no longer
   enumerated. They are replaced by a `broad_impact` row naming the module
   and its reach, so a near-full run driven by one helper such as
   `rabbit_ct_broker_helpers` becomes an explicit broad-impact signal
   instead of a silent balloon. A suite kept by an independent reason
   still survives, and the new `SuitePlan.broad_impact` field is additive
 * `RoundClearance` gains a `ZeroDomain` arm for batches where every
   candidate is inapplicable. Previously such batches resolved to `Clean`,
   making "all inapplicable, 0 tracked symbols" read as a low-risk signal
   when backhopper's dep and symbol domain never intersected the round at
   all. `ZeroDomain` exits 0 and `is_clean()` returns `true` (behavioral
   continuity), but `render_clearance` emits "all candidates are outside
   backhopper's dep and symbol scope; this verdict does not bound round
   risk" in place of the clean-round trust statement

### Bug Fixes

 * A bitstring type with an internal comma (e.g. `<<_:8, _:_*8>>`) is no
   longer split on that comma: a `-spec`, `-callback`, or `-type`
   argument keeps its arity, and a record field keeps its identity
   instead of fracturing into two fields. The Erlang extractor version is
   now 5, so `snapshots verify --all` flags snapshots generated before
   the fix

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
