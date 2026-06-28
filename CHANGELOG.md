# Changelog

## v0.20.0 (in development)

### Enhancements

 * Ambiguous SHA-prefix resolution disambiguates through gix's indexed
   `lookup_prefix` (a binary search over the pack and loose object
   indices) instead of scanning the whole object database
 * `GitRepo::has_commit` checks the object header instead of decoding
   the full commit

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
