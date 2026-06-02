// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::ffi::OsStr;
use std::time::Duration;

use backhopper_driver::{EnvInheritance, EnvOverlay, GlobalOptions};

#[test]
fn builder_sets_every_documented_field() {
    let opts = GlobalOptions::builder()
        .config_path("/x/backhopper.toml")
        .snapshot_dir("/x/snap")
        .repo_dir_path("/x/repo")
        .working_directory("/x")
        .non_interactive(true)
        .quiet(true)
        .verbose(false)
        .dry_run(true)
        .timeout(Duration::from_secs(7))
        .env(EnvOverlay::clear())
        .build();

    assert_eq!(
        opts.config_path.unwrap().to_string_lossy(),
        "/x/backhopper.toml"
    );
    assert_eq!(opts.snapshot_dir.unwrap().to_string_lossy(), "/x/snap");
    assert_eq!(opts.repo_dir_path.unwrap().to_string_lossy(), "/x/repo");
    assert_eq!(opts.working_directory.unwrap().to_string_lossy(), "/x");
    assert!(opts.non_interactive);
    assert!(opts.quiet);
    assert!(!opts.verbose);
    assert!(opts.dry_run);
    assert_eq!(opts.timeout, Some(Duration::from_secs(7)));
    assert_eq!(opts.env.inheritance, EnvInheritance::Clear);
}

#[test]
fn env_overlay_inherit_is_the_default_inheritance() {
    let overlay = EnvOverlay::default();
    assert_eq!(overlay.inheritance, EnvInheritance::Inherit);
}

#[test]
fn env_overlay_allow_list_carries_static_names() {
    let overlay = EnvOverlay::allow_list(&["HOME", "PATH"]);
    match overlay.inheritance {
        EnvInheritance::AllowList(names) => assert_eq!(names, &["HOME", "PATH"]),
        other => panic!("expected AllowList, got {other:?}"),
    }
}

#[test]
fn env_overlay_with_replaces_existing_key() {
    let overlay = EnvOverlay::inherit().with("FOO", "1").with("FOO", "2");
    assert_eq!(overlay.overrides.len(), 1);
    assert_eq!(
        overlay.overrides.get(OsStr::new("FOO")).unwrap(),
        OsStr::new("2"),
    );
}
