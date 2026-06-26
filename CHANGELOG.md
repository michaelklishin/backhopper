# Changelog

## v0.19.0 (in development)

### Enhancements

 * A path absent on the target while others are present is now a
   non-blocking `target_path_absent` reason, not silently dropped

### Bug Fixes

 * `?MACRO` and `#record` uses are no longer reported as undefined when
   the file includes an unreadable stdlib header (`eunit`, `logger.hrl`):
   the define set is incomplete, so the check skips them
 * Variable applications (`Fun(...)`), attribute forms (`-export(...)`),
   and `-spec` type names are no longer read as undefined local calls;
   the scanner skips them instead of re-lexing their tails

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
