// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `--summary-only` is being retired in favour of `--formatter summary`.
//! When the deprecated flag is passed, the CLI must surface a stderr
//! warning that names the replacement.

use crate::helpers::cli::{run, stderr};

#[test]
fn summary_only_flag_emits_a_deprecation_warning_to_stderr() {
    let a = run([
        "check",
        "patch",
        "--summary-only",
        "--project",
        "ra",
        "--tag",
        "v1.0.0",
        "--repo-dir-path",
        "/tmp/does-not-exist",
    ]);
    let err = stderr(&a);
    assert!(
        err.contains("--summary-only is deprecated"),
        "stderr must announce the deprecation: {err}"
    );
    assert!(
        err.contains("--formatter summary"),
        "stderr must point at the replacement: {err}"
    );
}
