// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::SymbolKind;
use backhopper_core::compat::patch::{Language, Patch};

const SCHEMA_DIFF: &str = "\
diff --git a/deps/rabbit/priv/schema/rabbit.schema b/deps/rabbit/priv/schema/rabbit.schema
--- a/deps/rabbit/priv/schema/rabbit.schema
+++ b/deps/rabbit/priv/schema/rabbit.schema
@@ -10,3 +10,8 @@
 % unchanged
+{translation, \"rabbit.definitions.ssl_options.password\",
+fun(Conf) ->
+    rabbit_cuttlefish:optionally_tagged_string(\"definitions.tls.password\", Conf)
+end}.
+
";

const SNIPPETS_DIFF: &str = "\
diff --git a/test/snippets/definitions.snippets b/test/snippets/definitions.snippets
--- a/test/snippets/definitions.snippets
+++ b/test/snippets/definitions.snippets
@@ -1,1 +1,2 @@
 definitions.tls.certfile = value
+definitions.tls.password.translates_via = rabbit_cuttlefish:optionally_tagged_binary(\"definitions.tls.password\", Conf)
";

#[test]
fn schema_files_are_tagged_as_cuttlefish() {
    let p = Patch::parse(SCHEMA_DIFF.as_bytes()).unwrap();
    assert_eq!(p.files.len(), 1);
    assert_eq!(p.files[0].language, Language::CuttlefishSchema);
}

#[test]
fn snippets_files_are_tagged_as_cuttlefish() {
    let p = Patch::parse(SNIPPETS_DIFF.as_bytes()).unwrap();
    assert_eq!(p.files.len(), 1);
    assert_eq!(p.files[0].language, Language::CuttlefishSchema);
}

const PARTIAL_DIFF: &str = "\
diff --git a/deps/rabbit/priv/schema/ssl_options.partial b/deps/rabbit/priv/schema/ssl_options.partial
--- a/deps/rabbit/priv/schema/ssl_options.partial
+++ b/deps/rabbit/priv/schema/ssl_options.partial
@@ -1,2 +1,4 @@
 % unchanged
+{translation, \"{{prefix}}.password\",
+fun(Conf) -> rabbit_cuttlefish:optionally_tagged_string(\"{{prefix}}.tls.password\", Conf) end}.
";

#[test]
fn partial_files_are_tagged_as_cuttlefish() {
    let p = Patch::parse(PARTIAL_DIFF.as_bytes()).unwrap();
    assert_eq!(p.files.len(), 1);
    assert_eq!(p.files[0].language, Language::CuttlefishSchema);
}

#[test]
fn analyze_picks_up_partial_mfa_references() {
    let p = Patch::parse(PARTIAL_DIFF.as_bytes()).unwrap().analyze();
    let calls: Vec<String> = p
        .referenced()
        .iter()
        .filter_map(|sym| match &sym.kind {
            SymbolKind::Function { mfa } => Some(mfa.to_string()),
            _ => None,
        })
        .collect();
    assert!(
        calls
            .iter()
            .any(|c| c == "rabbit_cuttlefish:optionally_tagged_string/2"),
        "expected partial-resident MFA in referenced list; got {calls:?}"
    );
}

#[test]
fn analyze_picks_up_schema_mfa_references() {
    let p = Patch::parse(SCHEMA_DIFF.as_bytes()).unwrap().analyze();
    let calls: Vec<String> = p
        .referenced()
        .iter()
        .filter_map(|sym| match &sym.kind {
            SymbolKind::Function { mfa } => Some(mfa.to_string()),
            _ => None,
        })
        .collect();
    assert!(
        calls
            .iter()
            .any(|c| c == "rabbit_cuttlefish:optionally_tagged_string/2"),
        "expected schema-resident MFA in referenced list; got {calls:?}"
    );
}

#[test]
fn analyze_picks_up_snippets_mfa_references() {
    let p = Patch::parse(SNIPPETS_DIFF.as_bytes()).unwrap().analyze();
    let calls: Vec<String> = p
        .referenced()
        .iter()
        .filter_map(|sym| match &sym.kind {
            SymbolKind::Function { mfa } => Some(mfa.to_string()),
            _ => None,
        })
        .collect();
    assert!(
        calls
            .iter()
            .any(|c| c == "rabbit_cuttlefish:optionally_tagged_binary/2"),
        "expected snippets-resident MFA in referenced list; got {calls:?}"
    );
}

#[test]
fn schema_analyzer_skips_removed_lines() {
    let diff = "\
diff --git a/rabbit.schema b/rabbit.schema
--- a/rabbit.schema
+++ b/rabbit.schema
@@ -1,2 +1,1 @@
-fun(C) -> stale_module:gone_function(C) end.
 % keep
";
    let p = Patch::parse(diff.as_bytes()).unwrap().analyze();
    let mentions_stale = p.referenced().iter().any(|sym| match &sym.kind {
        SymbolKind::Function { mfa } => mfa.module.as_str() == "stale_module",
        _ => false,
    });
    assert!(
        !mentions_stale,
        "removed lines must not contribute to referenced symbols"
    );
}
