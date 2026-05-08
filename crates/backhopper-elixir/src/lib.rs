//! Elixir source surface extractor.
//!
//! Reads `.ex`/`.exs` source and yields the `Module` records from
//! `backhopper-core`. Recognized:
//!  * `defmodule X.Y do ... end`         module names; nested modules become separate entries
//!  * `def name(args)`/`def name(args) do` exported functions
//!  * `defp ...`                          private (skipped)
//!  * `defmacro name(args)`               exported macros (recorded as exports)
//!  * `defmacrop ...`                     private macros (skipped)
//!  * `@callback name(...) :: rhs`        callback signatures
//!  * `@spec name(...) :: rhs`            specs
//!  * `@type name(args) :: rhs`           types
//!  * `@moduledoc false` / `@doc false`   visibility hints

pub mod extractor;

pub use extractor::{ElixirExtractor, ExtractError, ExtractedSource};
