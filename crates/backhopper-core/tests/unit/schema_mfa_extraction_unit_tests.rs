// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::str::FromStr;

use backhopper_core::compat::patch::{Patch, SourceKind};
use backhopper_core::model::names::Mfa;
use backhopper_core::model::symbol::{SymbolKind, SymbolRef};

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
    assert_eq!(p.files[0].language, SourceKind::CuttlefishSchema);
}

#[test]
fn snippets_files_are_tagged_as_cuttlefish() {
    let p = Patch::parse(SNIPPETS_DIFF.as_bytes()).unwrap();
    assert_eq!(p.files.len(), 1);
    assert_eq!(p.files[0].language, SourceKind::CuttlefishSchema);
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
    assert_eq!(p.files[0].language, SourceKind::CuttlefishSchema);
}

// Core no longer parses .schema fragments: that needs the whole file and the
// cuttlefish parser from a higher crate; references arrive via with_extra_references.
#[test]
fn analyze_leaves_schema_files_without_extracting_references() {
    let p = Patch::parse(SCHEMA_DIFF.as_bytes()).unwrap().analyze();
    assert!(p.referenced().is_empty());
}

#[test]
fn with_extra_references_folds_in_injected_references() {
    let p = Patch::parse(SCHEMA_DIFF.as_bytes()).unwrap().analyze();
    let mfa = Mfa::from_str("rabbit_cuttlefish:optionally_tagged_string/2").unwrap();
    let p = p.with_extra_references([SymbolRef::function(mfa)]);
    let calls: Vec<String> = p
        .referenced()
        .iter()
        .filter_map(|sym| match &sym.kind {
            SymbolKind::Function { mfa } => Some(mfa.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(calls, ["rabbit_cuttlefish:optionally_tagged_string/2"]);
}
