// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `extract_function_signatures` must not mistake non-calls for local
//! calls: a variable application's tail, an attribute name, or a spec
//! type. It must still find genuine calls in a function body.

use proptest::prelude::*;

use backhopper_core::compat::source_attributes::extract_function_signatures;

proptest! {
    // `Var(Args)` calls a variable: no emitted name may be a suffix of
    // the variable, the failure mode where `Fun(` became `un/2`.
    #[test]
    fn a_variable_tail_is_never_re_lexed_as_a_call(
        head in "[A-Z][a-z]{1,8}",
        arity in 0usize..4,
    ) {
        let args = vec!["ok"; arity].join(", ");
        let src = format!("g() -> {head}({args}).\n");
        let lower = head.to_ascii_lowercase();
        for s in extract_function_signatures(&src) {
            prop_assert!(
                s.name == "g" || !lower.ends_with(s.name.as_str()),
                "{} re-lexed from variable {head}",
                s.name
            );
        }
    }

    // A spec form holds type references, not calls: it emits nothing.
    #[test]
    fn a_spec_form_emits_no_signatures(
        name in "myf_[a-z]{0,6}",
        ty in prop::sample::select(vec!["map()", "term()", "any()", "integer()"]),
    ) {
        let src = format!("-spec {name}({ty}) -> {ty}.\n");
        prop_assert!(extract_function_signatures(&src).is_empty());
    }

    // A genuine call in a function body is still found: the suppression
    // above must not swallow real calls.
    #[test]
    fn a_body_call_is_still_found(name in "myf_[a-z0-9_]{1,8}") {
        let src = format!("g() -> {name}().\n");
        let sigs = extract_function_signatures(&src);
        prop_assert!(sigs.iter().any(|s| s.name == name && !s.is_definition));
    }

    // The attribute's closing `.` must hand scanning back: a call on the
    // following line is still found, never swallowed by the form.
    #[test]
    fn an_attribute_does_not_swallow_the_next_call(name in "myf_[a-z0-9_]{1,8}") {
        let src = format!("-spec g() -> ok.\ng() -> {name}().\n");
        let sigs = extract_function_signatures(&src);
        prop_assert!(sigs.iter().any(|s| s.name == name && !s.is_definition));
    }
}
