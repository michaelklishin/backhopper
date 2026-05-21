// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use time::OffsetDateTime;

use backhopper_core::model::names::{CommitSha, ProjectName, TagName};
use backhopper_core::model::snapshot::{Snapshot, SnapshotHeader, Visibility};
use backhopper_core::snapshot::{format, parser};
use backhopper_erlang::ErlangExtractor;

const RA_LIKE: &str = r#"
%% A representative `ra`-style module: behaviour, exports, callbacks, types.
-module(ra).
-behaviour(gen_statem).

-export([
         start/0,
         start/1,
         start_server/2,
         process_command/2,
         process_command/3,
         pipeline_command/2
        ]).

-export_type([range/0]).

-callback init(Conf :: machine_init_args()) -> state().
-callback apply(Meta :: command_meta_data(), command(), State) ->
   {State, reply(), effects()} | {State, reply()}.

-spec process_command(ServerId :: term(), Command :: term()) ->
    {ok, term(), term()} | {timeout, term()} | {error, term()}.

-type range() :: undefined | {ra:index(), ra:index()}.

-deprecated([{start_node, 2, "use start_server/2"}]).

-ifdef(TEST).
-export([test_helper/1]).
-endif.

start() -> ok.
start(_) -> ok.
start_server(_C, _S) -> ok.
process_command(_S, _C) -> ok.
process_command(_S, _C, _T) -> ok.
pipeline_command(_S, _C) -> ok.
"#;

#[test]
fn extracts_full_module_surface() {
    let ex = ErlangExtractor::default();
    let m = ex.extract_module(RA_LIKE).expect("module");
    assert_eq!(m.name.as_str(), "ra");
    let exports: Vec<String> = m
        .exports
        .iter()
        .map(|fa| format!("{}/{}", fa.name, fa.arity))
        .collect();
    assert!(exports.contains(&"start/0".to_owned()));
    assert!(exports.contains(&"start/1".to_owned()));
    assert!(exports.contains(&"start_server/2".to_owned()));
    assert!(exports.contains(&"process_command/2".to_owned()));
    assert!(exports.contains(&"process_command/3".to_owned()));
    assert!(exports.contains(&"pipeline_command/2".to_owned()));
    assert_eq!(m.behaviours.len(), 1);
    assert_eq!(m.behaviours[0].as_str(), "gen_statem");
    assert_eq!(m.export_types.len(), 1);
    assert_eq!(m.callbacks.len(), 2);
    assert_eq!(m.specs.len(), 1);
    assert_eq!(m.types.len(), 1);
    assert_eq!(m.deprecations.len(), 1);
    let dep = &m.deprecations[0];
    assert_eq!(
        dep.function.as_ref().map(|f| f.as_str()),
        Some("start_node")
    );
    assert!(matches!(
        m.visibility,
        Visibility::Public | Visibility::TestOnly
    ));
}

#[test]
fn snapshot_round_trip_for_realistic_module() {
    let ex = ErlangExtractor::default();
    let m = ex.extract_module(RA_LIKE).expect("module");
    let header = SnapshotHeader {
        project: ProjectName::new("ra").unwrap(),
        tag: TagName::new("v3.1.6").unwrap(),
        branch: None,
        commit: CommitSha::new("0".repeat(40)).unwrap(),
        scanned_paths: vec!["src".into()],
        apps_scanned: Vec::new(),
        generated_by: "backhopper test".into(),
        generated_at: OffsetDateTime::from_unix_timestamp(0).unwrap(),
    };
    let snap = Snapshot::from_extracted(header, vec![m], vec![]).into_canonical();
    let text = format::to_string(&snap).unwrap();
    let back = parser::parse(&text).expect("re-parse");
    assert_eq!(snap, back);
}
