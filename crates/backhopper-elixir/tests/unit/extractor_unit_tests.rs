// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::snapshot::Visibility;
use backhopper_elixir::ElixirExtractor;

const SIMPLE: &str = r#"
defmodule Khepri.Machine do
  @spec greet(String.t()) :: String.t()
  def greet(name), do: "hi " <> name

  defp helper(x), do: x

  defmacro debug(x) do
    quote do: IO.inspect(unquote(x))
  end
end
"#;

const NESTED: &str = r#"
defmodule Ra do
  defmodule Server do
    @callback handle(term()) :: :ok
    def init(x), do: x
  end
end
"#;

const TYPES: &str = r#"
defmodule TypesOnly do
  @type id :: pos_integer()
  @type pair(a, b) :: {a, b}
  @opaque token :: binary()
end
"#;

const HIDDEN: &str = r#"
defmodule Internal do
  @moduledoc false
  def start, do: :ok
end
"#;

#[test]
fn extracts_def_and_defmacro_as_exports_skips_defp() {
    let ex = ElixirExtractor::default();
    let modules = ex.extract_modules(SIMPLE);
    let m = modules
        .iter()
        .find(|m| m.name.as_str() == "Khepri.Machine")
        .expect("module");
    let exports: Vec<_> = m
        .exports
        .iter()
        .map(|fa| format!("{}/{}", fa.name, fa.arity))
        .collect();
    assert!(exports.iter().any(|e| e == "greet/1"));
    assert!(exports.iter().any(|e| e == "debug/1"));
    assert!(!exports.iter().any(|e| e == "helper/1"));
}

#[test]
fn nested_modules_are_namespaced_with_parent() {
    let ex = ElixirExtractor::default();
    let modules = ex.extract_modules(NESTED);
    let names: Vec<_> = modules.iter().map(|m| m.name.to_string()).collect();
    assert!(names.contains(&"Ra".to_owned()));
    assert!(names.contains(&"Ra.Server".to_owned()));
    let inner = modules
        .iter()
        .find(|m| m.name.as_str() == "Ra.Server")
        .expect("inner module");
    assert!(inner.callbacks.iter().any(|c| c.name.as_str() == "handle"));
    assert!(inner.exports.iter().any(|fa| fa.name.as_str() == "init"));
}

#[test]
fn types_collected_with_arity() {
    let ex = ElixirExtractor::default();
    let modules = ex.extract_modules(TYPES);
    let m = &modules[0];
    let types: Vec<_> = m
        .types
        .iter()
        .map(|t| format!("{}/{}", t.name, t.arity))
        .collect();
    assert!(types.iter().any(|t| t == "id/0"));
    assert!(types.iter().any(|t| t == "pair/2"));
}

#[test]
fn moduledoc_false_marks_module_hidden() {
    let ex = ElixirExtractor::default();
    let modules = ex.extract_modules(HIDDEN);
    assert_eq!(modules[0].visibility, Visibility::Hidden);
}

#[test]
fn heredoc_examples_are_not_treated_as_modules() {
    let source = r#"
defmodule Real do
  @moduledoc """
  defmodule FromExample do
    def example, do: :ok
  end
  """
  def real, do: :ok
end
"#;
    let ex = ElixirExtractor::default();
    let modules = ex.extract_modules(source);
    assert_eq!(modules.len(), 1);
    assert_eq!(modules[0].name.as_str(), "Real");
    assert!(
        modules[0]
            .exports
            .iter()
            .any(|fa| fa.name.as_str() == "real")
    );
}

#[test]
fn def_with_no_args_yields_arity_zero() {
    let ex = ElixirExtractor::default();
    let modules = ex.extract_modules("defmodule Khepri do\n  def is_empty, do: :ok\nend\n");
    assert!(
        modules[0]
            .exports
            .iter()
            .any(|fa| fa.name.as_str() == "is_empty" && fa.arity.get() == 0)
    );
}

fn exports_of(source: &str, module: &str) -> Vec<String> {
    let ex = ElixirExtractor::default();
    ex.extract_modules(source)
        .iter()
        .find(|m| m.name.as_str() == module)
        .expect("module")
        .exports
        .iter()
        .map(|fa| format!("{}/{}", fa.name, fa.arity))
        .collect()
}

// a ?" char literal in a default argument must not open a string and collapse the arity to zero
#[test]
fn char_literal_quote_in_default_arg_keeps_arity() {
    let source = "defmodule Osiris.Log do\n  def encode(sep \\\\ ?\", b, c), do: :ok\nend\n";
    let exports = exports_of(source, "Osiris.Log");
    assert!(exports.iter().any(|e| e == "encode/3"), "got {exports:?}");
}

// a ?, char literal must not be counted as an argument separator
#[test]
fn char_literal_comma_in_default_arg_does_not_inflate_arity() {
    let source = "defmodule Ra.Log do\n  def decode(sep \\\\ ?,, b), do: :ok\nend\n";
    let exports = exports_of(source, "Ra.Log");
    assert!(exports.iter().any(|e| e == "decode/2"), "got {exports:?}");
}

// the trailing ? in a predicate name like valid? is not a char literal: arity stays correct
#[test]
fn predicate_name_is_not_mistaken_for_a_char_literal() {
    let source = "defmodule Khepri.Path do\n  def valid?(a, b), do: true\nend\n";
    let exports = exports_of(source, "Khepri.Path");
    assert!(exports.iter().any(|e| e == "valid?/2"), "got {exports:?}");
}

#[test]
fn callback_with_complex_args_yields_correct_arity() {
    let source = r#"
defmodule Ra.Machine do
  @callback handle(conn :: term(), opts :: keyword()) :: term()
end
"#;
    let ex = ElixirExtractor::default();
    let modules = ex.extract_modules(source);
    let cb = &modules[0].callbacks[0];
    assert_eq!(cb.name.as_str(), "handle");
    assert_eq!(cb.arity.get(), 2);
}
