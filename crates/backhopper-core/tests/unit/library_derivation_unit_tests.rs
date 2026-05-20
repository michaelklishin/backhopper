// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;
use std::path::PathBuf;

use backhopper_core::app_src::AppSrcSpec;
use backhopper_core::model::names::{ApplicationName, ModuleName};
use backhopper_core::suites::derive_library_apps;

fn spec(name: &str, deps: &[&str]) -> AppSrcSpec {
    AppSrcSpec {
        path: PathBuf::from(format!("deps/{name}/src/{name}.app.src")),
        name: ApplicationName::new(name).unwrap(),
        vsn: None,
        modules: BTreeSet::<ModuleName>::new(),
        applications: deps
            .iter()
            .map(|d| ApplicationName::new(*d).unwrap())
            .collect(),
    }
}

#[test]
fn no_apps_yields_no_libraries() {
    assert!(derive_library_apps(&[]).is_empty());
}

#[test]
fn app_depended_on_by_one_other_is_not_a_library() {
    let apps = vec![spec("rabbit", &[]), spec("consumer", &["rabbit"])];
    assert!(derive_library_apps(&apps).is_empty());
}

#[test]
fn app_depended_on_by_two_others_is_a_library() {
    let apps = vec![
        spec("rabbit", &[]),
        spec("mgmt", &["rabbit"]),
        spec("stomp", &["rabbit"]),
    ];
    let libs = derive_library_apps(&apps);
    assert_eq!(libs.len(), 1);
    assert_eq!(libs[0].as_str(), "rabbit");
}

#[test]
fn out_of_tree_deps_are_ignored_for_the_count() {
    let apps = vec![
        spec("rabbit", &["kernel", "stdlib"]),
        spec("mgmt", &["rabbit", "kernel"]),
        spec("stomp", &["rabbit", "stdlib"]),
    ];
    let libs = derive_library_apps(&apps);
    let names: Vec<&str> = libs.iter().map(|n| n.as_str()).collect();
    // kernel and stdlib are not in-tree -- they don't get counted.
    assert_eq!(names, vec!["rabbit"]);
}

#[test]
fn self_reference_does_not_count() {
    let apps = vec![spec("rabbit", &["rabbit"]), spec("mgmt", &["rabbit"])];
    // mgmt counts once; rabbit listing itself doesn't push it to 2.
    assert!(derive_library_apps(&apps).is_empty());
}

#[test]
fn multiple_libraries_returned_alphabetically() {
    let apps = vec![
        spec("rabbit", &[]),
        spec("rabbit_common", &[]),
        spec("a", &["rabbit", "rabbit_common"]),
        spec("b", &["rabbit", "rabbit_common"]),
    ];
    let libs = derive_library_apps(&apps);
    let names: Vec<&str> = libs.iter().map(|n| n.as_str()).collect();
    assert_eq!(names, vec!["rabbit", "rabbit_common"]);
}
