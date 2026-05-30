// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Snapshot fixtures captured from real ra, khepri, osiris, cowboy,
//! seshat, ranch (Erlang), and plug (Elixir) checkouts. They serve as
//! a regression corpus for the parser and store, and as a known-good
//! source for the `api lookup` query.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use backhopper_core::model::names::{Mfa, ProjectName, TagName};
use backhopper_core::snapshot::{format, parser};
use backhopper_core::store::SnapshotStore;

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("real_world")
}

const PROJECTS: &[(&str, usize)] = &[
    ("ra", 10),
    ("khepri", 10),
    ("osiris", 10),
    ("cowboy", 5),
    ("plug", 10),
    ("seshat", 4),
    ("ranch", 3),
];

#[test]
fn every_fixture_parses_canonically() {
    let root = fixtures_root();
    let mut total = 0usize;
    for (project, _) in PROJECTS {
        let dir = root.join(project);
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let text = std::fs::read_to_string(&path).unwrap();
            let snap = parser::parse(&text)
                .unwrap_or_else(|e| panic!("parse failed for {}: {:?}", path.display(), e));
            assert_eq!(snap.header().project.as_str(), *project);
            total += 1;
        }
    }
    let expected: usize = PROJECTS.iter().map(|(_, n)| *n).sum();
    assert_eq!(total, expected);
}

#[test]
fn store_lists_tags_for_each_project() {
    let store = SnapshotStore::open(fixtures_root()).unwrap();
    for (project, expected) in PROJECTS {
        let p = ProjectName::new(*project).unwrap();
        let tags = store.list_tags(&p).unwrap();
        assert_eq!(
            tags.len(),
            *expected,
            "expected {} tags for {}, found {}",
            expected,
            project,
            tags.len()
        );
    }
}

#[test]
fn store_round_trips_every_snapshot() {
    let store = SnapshotStore::open(fixtures_root()).unwrap();
    for (project, _) in PROJECTS {
        let p = ProjectName::new(*project).unwrap();
        for tag in store.list_tags(&p).unwrap() {
            let snap = store.read(&p, &tag).unwrap();
            let serialized = format::to_string(&snap).unwrap();
            let reparsed = parser::parse(&serialized)
                .unwrap_or_else(|e| panic!("re-parse failed for {project} {tag}: {e:?}"));
            assert_eq!(snap, reparsed, "round-trip mismatch for {project} {tag}");
        }
    }
}

#[test]
fn ra_v3_1_6_exports_well_known_functions() {
    let store = SnapshotStore::open(fixtures_root()).unwrap();
    let p = ProjectName::new("ra").unwrap();
    let t = TagName::new("v3.1.6").unwrap();
    let snap = store.read(&p, &t).unwrap();
    let mfas = [
        "ra:start/0",
        "ra:start_cluster/3",
        "ra:start_server/5",
        "ra:process_command/3",
    ];
    for m in mfas {
        let mfa = Mfa::from_str(m).unwrap();
        assert!(
            snap.lookup_export(&mfa.module, &mfa.function, mfa.arity),
            "ra v3.1.6 should export {m}"
        );
    }
}

#[test]
fn cowboy_2_14_x_exports_well_known_functions() {
    let store = SnapshotStore::open(fixtures_root()).unwrap();
    let p = ProjectName::new("cowboy").unwrap();
    for tag_str in ["2.14.0", "2.14.1", "2.14.2"] {
        let t = TagName::new(tag_str).unwrap();
        let snap = store.read(&p, &t).unwrap();
        let mfa = Mfa::from_str("cowboy_req:reply/4").unwrap();
        assert!(
            snap.lookup_export(&mfa.module, &mfa.function, mfa.arity),
            "cowboy {tag_str} should export cowboy_req:reply/4"
        );
    }
}

// 2.12.0 is the cowboy pin for the 3.13.x backports branch; 2.13.0 is
// the pin for 4.0.x and 4.1.x. 2.14.x is the next-minor successor.
#[test]
fn cowboy_fixtures_cover_pinned_versions() {
    let store = SnapshotStore::open(fixtures_root()).unwrap();
    let p = ProjectName::new("cowboy").unwrap();
    let tags: Vec<String> = store
        .list_tags(&p)
        .unwrap()
        .iter()
        .map(|s| s.to_string())
        .collect();
    for expected in ["2.12.0", "2.13.0", "2.14.0", "2.14.1", "2.14.2"] {
        assert!(tags.iter().any(|n| n == expected), "missing tag {expected}");
    }
}

// QUIC support landed at 2.13.0: `cowboy:start_quic/3` was added.
#[test]
fn cowboy_start_quic_appears_at_2_13_0() {
    let store = SnapshotStore::open(fixtures_root()).unwrap();
    let p = ProjectName::new("cowboy").unwrap();
    let mfa = Mfa::from_str("cowboy:start_quic/3").unwrap();
    let v2_12 = store.read(&p, &TagName::new("2.12.0").unwrap()).unwrap();
    assert!(!v2_12.lookup_export(&mfa.module, &mfa.function, mfa.arity));
    for tag_str in ["2.13.0", "2.14.0", "2.14.1", "2.14.2"] {
        let snap = store.read(&p, &TagName::new(tag_str).unwrap()).unwrap();
        assert!(
            snap.lookup_export(&mfa.module, &mfa.function, mfa.arity),
            "cowboy {tag_str} should export cowboy:start_quic/3"
        );
    }
}

// WebTransport support landed at 2.14.0: a whole new `cowboy_webtransport`
// module appeared with `upgrade/{4,5}`, `info/3`, and `terminate/3`.
#[test]
fn cowboy_webtransport_module_appears_at_2_14_0() {
    let store = SnapshotStore::open(fixtures_root()).unwrap();
    let p = ProjectName::new("cowboy").unwrap();
    for tag_str in ["2.12.0", "2.13.0"] {
        let snap = store.read(&p, &TagName::new(tag_str).unwrap()).unwrap();
        assert!(
            !snap
                .modules()
                .iter()
                .any(|m| m.name.as_str() == "cowboy_webtransport"),
            "cowboy {tag_str} should not have cowboy_webtransport"
        );
    }
    for tag_str in ["2.14.0", "2.14.1", "2.14.2"] {
        let snap = store.read(&p, &TagName::new(tag_str).unwrap()).unwrap();
        let module = snap
            .modules()
            .iter()
            .find(|m| m.name.as_str() == "cowboy_webtransport")
            .unwrap_or_else(|| panic!("cowboy {tag_str} should have cowboy_webtransport"));
        for (fun, arity) in [
            ("upgrade", 4u8),
            ("upgrade", 5),
            ("info", 3),
            ("terminate", 3),
        ] {
            assert!(
                module
                    .exports
                    .iter()
                    .any(|fa| fa.name.as_str() == fun && fa.arity.get() == arity),
                "cowboy {tag_str} cowboy_webtransport should export {fun}/{arity}"
            );
        }
    }
}

// Regression guard for callback extraction across every supported tag.
#[test]
fn cowboy_handler_init_callback_is_captured() {
    let store = SnapshotStore::open(fixtures_root()).unwrap();
    let p = ProjectName::new("cowboy").unwrap();
    for tag_str in ["2.12.0", "2.13.0", "2.14.0", "2.14.1", "2.14.2"] {
        let snap = store.read(&p, &TagName::new(tag_str).unwrap()).unwrap();
        let handler = snap
            .modules()
            .iter()
            .find(|m| m.name.as_str() == "cowboy_handler")
            .unwrap_or_else(|| panic!("cowboy {tag_str} should have cowboy_handler"));
        assert!(
            handler
                .callbacks
                .iter()
                .any(|c| c.name.as_str() == "init" && c.arity.get() == 2),
            "cowboy {tag_str} cowboy_handler:init/2 callback expected"
        );
    }
}

#[test]
fn osiris_v1_13_1_exports_well_known_functions() {
    let store = SnapshotStore::open(fixtures_root()).unwrap();
    let p = ProjectName::new("osiris").unwrap();
    let t = TagName::new("v1.13.1").unwrap();
    let snap = store.read(&p, &t).unwrap();
    let mfa = Mfa::from_str("osiris_log:init/1").unwrap();
    assert!(
        snap.lookup_export(&mfa.module, &mfa.function, mfa.arity)
            || snap
                .modules()
                .iter()
                .any(|m| m.name.as_str() == "osiris_log"),
        "osiris v1.13.1 should at least have osiris_log module"
    );
}

#[test]
fn plug_v1_19_1_has_callback_and_export() {
    let store = SnapshotStore::open(fixtures_root()).unwrap();
    let p = ProjectName::new("plug").unwrap();
    let t = TagName::new("v1.19.1").unwrap();
    let snap = store.read(&p, &t).unwrap();
    let plug_module = snap
        .modules()
        .iter()
        .find(|m| m.name.as_str() == "Plug")
        .expect("Plug module present in v1.19.1");
    assert!(
        plug_module
            .callbacks
            .iter()
            .any(|c| c.name.as_str() == "init" && c.arity.get() == 1),
        "Plug.init/1 callback expected"
    );
    assert!(
        plug_module
            .callbacks
            .iter()
            .any(|c| c.name.as_str() == "call" && c.arity.get() == 2),
        "Plug.call/2 callback expected"
    );
    assert!(
        plug_module
            .exports
            .iter()
            .any(|fa| fa.name.as_str() == "run" && fa.arity.get() == 3),
        "Plug.run/3 export expected"
    );
}

#[test]
fn plug_router_module_present_in_recent_tag() {
    let store = SnapshotStore::open(fixtures_root()).unwrap();
    let p = ProjectName::new("plug").unwrap();
    let t = TagName::new("v1.19.1").unwrap();
    let snap = store.read(&p, &t).unwrap();
    assert!(
        snap.modules()
            .iter()
            .any(|m| m.name.as_str() == "Plug.Router"),
        "Plug.Router module expected"
    );
}

#[test]
fn khepri_v0_18_0_has_command_module() {
    let store = SnapshotStore::open(fixtures_root()).unwrap();
    let p = ProjectName::new("khepri").unwrap();
    let t = TagName::new("v0.18.0").unwrap();
    let snap = store.read(&p, &t).unwrap();
    assert!(
        snap.modules().iter().any(|m| m.name.as_str() == "khepri"),
        "khepri v0.18.0 should have a `khepri` module"
    );
}

#[test]
fn seshat_v1_0_0_exports_core_counter_api() {
    let store = SnapshotStore::open(fixtures_root()).unwrap();
    let p = ProjectName::new("seshat").unwrap();
    let t = TagName::new("v1.0.0").unwrap();
    let snap = store.read(&p, &t).unwrap();
    let mfas = [
        "seshat:new_group/1",
        "seshat:delete_group/1",
        "seshat:new/3",
        "seshat:new/4",
        "seshat:fetch/2",
        "seshat:delete/2",
        "seshat:counters/1",
        "seshat:counters/2",
        "seshat:counters/3",
        "seshat:format/1",
        "seshat:format/2",
        "seshat:prom_format/2",
        "seshat:prom_format/3",
    ];
    for m in mfas {
        let mfa = Mfa::from_str(m).unwrap();
        assert!(
            snap.lookup_export(&mfa.module, &mfa.function, mfa.arity),
            "seshat v1.0.0 should export {m}"
        );
    }
}

// `seshat:prom_format/2,3` were added at v1.0.0 alongside the
// Prometheus support refactor (PR #15). v0.6.x must not expose them.
#[test]
fn seshat_prom_format_appears_at_v1_0_0() {
    let store = SnapshotStore::open(fixtures_root()).unwrap();
    let p = ProjectName::new("seshat").unwrap();
    let mfa = Mfa::from_str("seshat:prom_format/2").unwrap();
    for tag_str in ["v0.6.0", "v0.6.1"] {
        let snap = store.read(&p, &TagName::new(tag_str).unwrap()).unwrap();
        assert!(
            !snap.lookup_export(&mfa.module, &mfa.function, mfa.arity),
            "seshat {tag_str} should not yet export prom_format/2"
        );
    }
    for tag_str in ["v1.0.0", "v1.0.1"] {
        let snap = store.read(&p, &TagName::new(tag_str).unwrap()).unwrap();
        assert!(
            snap.lookup_export(&mfa.module, &mfa.function, mfa.arity),
            "seshat {tag_str} should export prom_format/2"
        );
    }
}

// `overview/1,2` existed up to v0.6.1 and was removed in favour of
// `counters/1,2,3` for v1.0.0 (commit c55f204).
#[test]
fn seshat_overview_function_removed_at_v1_0_0() {
    let store = SnapshotStore::open(fixtures_root()).unwrap();
    let p = ProjectName::new("seshat").unwrap();
    let overview_1 = Mfa::from_str("seshat:overview/1").unwrap();
    let overview_2 = Mfa::from_str("seshat:overview/2").unwrap();
    for tag_str in ["v0.6.0", "v0.6.1"] {
        let snap = store.read(&p, &TagName::new(tag_str).unwrap()).unwrap();
        assert!(
            snap.lookup_export(&overview_1.module, &overview_1.function, overview_1.arity),
            "seshat {tag_str} should export overview/1"
        );
        assert!(
            snap.lookup_export(&overview_2.module, &overview_2.function, overview_2.arity),
            "seshat {tag_str} should export overview/2"
        );
    }
    for tag_str in ["v1.0.0", "v1.0.1"] {
        let snap = store.read(&p, &TagName::new(tag_str).unwrap()).unwrap();
        assert!(
            !snap.lookup_export(&overview_1.module, &overview_1.function, overview_1.arity),
            "seshat {tag_str} should not export overview/1"
        );
        assert!(
            !snap.lookup_export(&overview_2.module, &overview_2.function, overview_2.arity),
            "seshat {tag_str} should not export overview/2"
        );
    }
}

// `counters/1` was added at v1.0.0 alongside the `overview` removal
// (commit a858276). v0.6.x exposes only counters/2,3.
#[test]
fn seshat_counters_1_added_at_v1_0_0() {
    let store = SnapshotStore::open(fixtures_root()).unwrap();
    let p = ProjectName::new("seshat").unwrap();
    let counters_1 = Mfa::from_str("seshat:counters/1").unwrap();
    let counters_2 = Mfa::from_str("seshat:counters/2").unwrap();
    let counters_3 = Mfa::from_str("seshat:counters/3").unwrap();
    let v0_6_1 = store.read(&p, &TagName::new("v0.6.1").unwrap()).unwrap();
    assert!(!v0_6_1.lookup_export(&counters_1.module, &counters_1.function, counters_1.arity));
    assert!(v0_6_1.lookup_export(&counters_2.module, &counters_2.function, counters_2.arity));
    assert!(v0_6_1.lookup_export(&counters_3.module, &counters_3.function, counters_3.arity));
    let v1_0_0 = store.read(&p, &TagName::new("v1.0.0").unwrap()).unwrap();
    assert!(v1_0_0.lookup_export(&counters_1.module, &counters_1.function, counters_1.arity));
    assert!(v1_0_0.lookup_export(&counters_2.module, &counters_2.function, counters_2.arity));
    assert!(v1_0_0.lookup_export(&counters_3.module, &counters_3.function, counters_3.arity));
}

// Fixture window: v0.6.1 is the pin for every active RabbitMQ
// backports branch (3.13.x, 4.0.x, 4.1.x); v1.0.x is its successor.
#[test]
fn seshat_fixtures_cover_pinned_and_next_versions() {
    let store = SnapshotStore::open(fixtures_root()).unwrap();
    let p = ProjectName::new("seshat").unwrap();
    let tags: Vec<String> = store
        .list_tags(&p)
        .unwrap()
        .iter()
        .map(|s| s.to_string())
        .collect();
    for expected in ["v0.6.0", "v0.6.1", "v1.0.0", "v1.0.1"] {
        assert!(tags.iter().any(|n| n == expected), "missing tag {expected}");
    }
}

// Spec strings should be captured intact, not mangled by the writer.
#[test]
fn seshat_specs_round_trip_cleanly() {
    let store = SnapshotStore::open(fixtures_root()).unwrap();
    let p = ProjectName::new("seshat").unwrap();
    let v1 = store.read(&p, &TagName::new("v1.0.0").unwrap()).unwrap();
    let seshat_module = v1
        .modules()
        .iter()
        .find(|m| m.name.as_str() == "seshat")
        .expect("seshat module present");
    let new_group_spec = seshat_module
        .specs
        .iter()
        .find(|s| s.name.as_str() == "new_group" && s.arity.get() == 1)
        .expect("spec new_group/1 should be captured");
    assert!(
        new_group_spec.signature.contains("group()"),
        "new_group/1 spec should mention group(): {}",
        new_group_spec.signature
    );
    assert!(
        new_group_spec.signature.contains("group_ref()"),
        "new_group/1 spec should mention group_ref(): {}",
        new_group_spec.signature
    );
}

// Ranch 2.1.0 is pinned by the 3.13.x and 4.0.x backports branches;
// 2.2.0 is pinned by 4.1.x. 2.0.0 is included as the major-version base.
#[test]
fn ranch_fixtures_cover_pinned_versions() {
    let store = SnapshotStore::open(fixtures_root()).unwrap();
    let p = ProjectName::new("ranch").unwrap();
    let tags: Vec<String> = store
        .list_tags(&p)
        .unwrap()
        .iter()
        .map(|s| s.to_string())
        .collect();
    for expected in ["2.0.0", "2.1.0", "2.2.0"] {
        assert!(tags.iter().any(|n| n == expected), "missing tag {expected}");
    }
}

#[test]
fn ranch_2_2_0_exports_core_listener_api() {
    let store = SnapshotStore::open(fixtures_root()).unwrap();
    let p = ProjectName::new("ranch").unwrap();
    let t = TagName::new("2.2.0").unwrap();
    let snap = store.read(&p, &t).unwrap();
    let mfas = [
        "ranch:start_listener/5",
        "ranch:stop_listener/1",
        "ranch:suspend_listener/1",
        "ranch:resume_listener/1",
        "ranch:handshake/1",
        "ranch:handshake/2",
        "ranch:recv_proxy_header/2",
        "ranch:get_port/1",
        "ranch:set_max_connections/2",
        "ranch:info/0",
        "ranch:info/1",
    ];
    for m in mfas {
        let mfa = Mfa::from_str(m).unwrap();
        assert!(
            snap.lookup_export(&mfa.module, &mfa.function, mfa.arity),
            "ranch 2.2.0 should export {m}"
        );
    }
}

// `format_error/1` landed in 2.2.0 on `ranch_tcp`, `ranch_ssl`, and as an optional callback on `ranch_transport`
#[test]
fn ranch_format_error_added_in_2_2_0() {
    let store = SnapshotStore::open(fixtures_root()).unwrap();
    let p = ProjectName::new("ranch").unwrap();
    let in_tcp = Mfa::from_str("ranch_tcp:format_error/1").unwrap();
    let in_ssl = Mfa::from_str("ranch_ssl:format_error/1").unwrap();
    for tag_str in ["2.0.0", "2.1.0"] {
        let snap = store.read(&p, &TagName::new(tag_str).unwrap()).unwrap();
        assert!(!snap.lookup_export(&in_tcp.module, &in_tcp.function, in_tcp.arity));
        assert!(!snap.lookup_export(&in_ssl.module, &in_ssl.function, in_ssl.arity));
    }
    let v2_2 = store.read(&p, &TagName::new("2.2.0").unwrap()).unwrap();
    assert!(v2_2.lookup_export(&in_tcp.module, &in_tcp.function, in_tcp.arity));
    assert!(v2_2.lookup_export(&in_ssl.module, &in_ssl.function, in_ssl.arity));
    let transport = v2_2
        .modules()
        .iter()
        .find(|m| m.name.as_str() == "ranch_transport")
        .expect("ranch_transport module present");
    assert!(
        transport
            .optional_callbacks
            .iter()
            .any(|c| c.name.as_str() == "format_error" && c.arity.get() == 1),
        "ranch_transport should declare optional callback format_error/1"
    );
}

#[test]
fn ranch_compat_normalize_alarms_option_appears_in_2_2_0() {
    let store = SnapshotStore::open(fixtures_root()).unwrap();
    let p = ProjectName::new("ranch").unwrap();
    let mfa = Mfa::from_str("ranch:compat_normalize_alarms_option/1").unwrap();
    for tag_str in ["2.0.0", "2.1.0"] {
        let snap = store.read(&p, &TagName::new(tag_str).unwrap()).unwrap();
        assert!(!snap.lookup_export(&mfa.module, &mfa.function, mfa.arity));
    }
    let v2_2 = store.read(&p, &TagName::new("2.2.0").unwrap()).unwrap();
    assert!(v2_2.lookup_export(&mfa.module, &mfa.function, mfa.arity));
}

#[test]
fn ranch_protocol_behaviour_callback_is_captured() {
    let store = SnapshotStore::open(fixtures_root()).unwrap();
    let p = ProjectName::new("ranch").unwrap();
    let t = TagName::new("2.2.0").unwrap();
    let snap = store.read(&p, &t).unwrap();
    let protocol = snap
        .modules()
        .iter()
        .find(|m| m.name.as_str() == "ranch_protocol")
        .expect("ranch_protocol module present");
    assert!(
        protocol
            .callbacks
            .iter()
            .any(|c| c.name.as_str() == "start_link" && c.arity.get() == 3),
        "ranch_protocol:start_link/3 callback expected"
    );
}
