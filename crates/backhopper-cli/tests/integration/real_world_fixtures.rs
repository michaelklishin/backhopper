//! Snapshot fixtures captured from real ra, khepri, osiris, and cowboy
//! checkouts: the 10 most recent tags of each. They serve as a regression
//! corpus for the parser and store, and as a known-good source for the
//! `api lookup` query.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use backhopper_core::model::names::{Mfa, ProjectName, TagName};
use backhopper_core::snapshot::parser;
use backhopper_core::store::SnapshotStore;

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("real_world")
}

fn project_dirs() -> [&'static str; 4] {
    ["ra", "khepri", "osiris", "cowboy"]
}

#[test]
fn every_fixture_parses_canonically() {
    let root = fixtures_root();
    let mut total = 0usize;
    for project in project_dirs() {
        let dir = root.join(project);
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let text = std::fs::read_to_string(&path).unwrap();
            let snap = parser::parse(&text)
                .unwrap_or_else(|e| panic!("parse failed for {}: {:?}", path.display(), e));
            assert_eq!(snap.header().project.as_str(), project);
            total += 1;
        }
    }
    assert_eq!(total, 40, "expected 40 fixtures, found {}", total);
}

#[test]
fn store_lists_tags_for_each_project() {
    let store = SnapshotStore::open(fixtures_root()).unwrap();
    for project in project_dirs() {
        let p = ProjectName::new(project).unwrap();
        let tags = store.list_tags(&p).unwrap();
        assert_eq!(
            tags.len(),
            10,
            "expected 10 tags for {}, found {}",
            project,
            tags.len()
        );
    }
}

#[test]
fn store_round_trips_every_snapshot() {
    use backhopper_core::snapshot::format;
    let store = SnapshotStore::open(fixtures_root()).unwrap();
    for project in project_dirs() {
        let p = ProjectName::new(project).unwrap();
        for tag in store.list_tags(&p).unwrap() {
            let snap = store.read(&p, &tag).unwrap();
            let serialized = format::to_string(&snap).unwrap();
            let reparsed = parser::parse(&serialized)
                .unwrap_or_else(|e| panic!("re-parse failed for {} {}: {:?}", project, tag, e));
            assert_eq!(
                snap, reparsed,
                "round-trip mismatch for {} {}",
                project, tag
            );
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
            "ra v3.1.6 should export {}",
            m
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
            "cowboy {} should export cowboy_req:reply/4",
            tag_str
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
