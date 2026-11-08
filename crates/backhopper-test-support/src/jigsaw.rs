// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! A configuration with one `[[host]]`, shared by the `jigsaw` tests.

pub const HOST_CONFIG_TOML: &str = r#"
config_version = 1

[[project]]
name = "rabbitmq-server"
git_url = "/srv/checkouts/rabbitmq-server.git"
layout = "multi_app"
app_roots = ["deps/*"]
exclude_apps = ["rabbitmq_cli"]
excluded_subdirs = ["test"]
manifest = "erlang_mk"

[[project]]
name = "ra"
git_url = "/srv/checkouts/ra.git"

[[project]]
name = "khepri"
git_url = "/srv/checkouts/khepri.git"

[[project]]
name = "otp"
git_url = "/srv/checkouts/otp.git"
layout = "erlang_otp"

[[host]]
name = "rabbitmq"
project = "rabbitmq-server"
otp_project = "otp"
series_tag_pattern = "v{series}.*"
erlang_majors = [27, 28]

[host.pins]
source = "erlang_mk"
file_path = "rabbitmq-components.mk"

[host.release_roots]
apps = ["rabbit"]
makefile_variable = { file_path = "plugins.mk", variable = "PLUGINS" }

[host.erlang_support]
default = "[27.0,30.0)"

[host.erlang_support.releases]
"4.2.0" = "[26.2,29.0)"
"4.2.9" = "[27.0,29.0)"
"#;
