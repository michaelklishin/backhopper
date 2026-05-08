# Changelog

## v0.1.0 (in development)

Initial release.

### Enhancements

 * Erlang public API snapshot extraction per git tag
 * Deterministic, canonically-ordered text snapshot file format
 * Type-state-enforced snapshot pipeline (`Unsorted` → `Canonical`)
 * `gix`-based git access — no shelling out to `git`, no `tar`
 * `compatibility patch` / `commit` / `range` against a single pin or a series
 * CLI command groups: `projects`, `series`, `snapshots`, `api`,
   `compatibility`, `config`, `completions`, `version`
 * JSON and text output formats via `--formatter`
 * Shell completions for bash, zsh, fish, nushell, and pwsh
