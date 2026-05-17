use std::path::PathBuf;

use tempfile::TempDir;

use backhopper_core::config::Config;
use backhopper_core::model::names::{ProjectName, SeriesName};

const SAMPLE: &str = r#"
config_version = 1

[defaults]
snapshot_dir = "snapshots"
fallback_branch = "main"
scan_paths = ["src/**/*.erl", "include/**/*.hrl"]

[[project]]
name = "ra"
git_url = "/tmp/ra.git"

[[project]]
name = "cowboy"
git_url = "/tmp/cowboy.git"
tag_prefix = ""

[[series]]
name = "rabbitmq-4.2"
pins = [
  { project = "ra", tag = "v3.1.6" },
  { project = "cowboy", tag = "2.13.0" },
]
"#;

fn write_config(tmp: &TempDir, body: &str) -> PathBuf {
    let path = tmp.path().join("backhopper.toml");
    std::fs::write(&path, body).unwrap();
    path
}

#[test]
fn config_loads_projects_and_series() {
    let tmp = TempDir::new().unwrap();
    let path = write_config(&tmp, SAMPLE);
    let cfg = Config::load(&path).unwrap();
    assert_eq!(cfg.projects.len(), 2);
    assert_eq!(cfg.series.len(), 1);
    let ra = cfg.project(&ProjectName::new("ra").unwrap()).unwrap();
    assert_eq!(ra.tag_prefix, "v");
    let cowboy = cfg.project(&ProjectName::new("cowboy").unwrap()).unwrap();
    assert_eq!(cowboy.tag_prefix, "");
    let s = cfg
        .series_by_name(&SeriesName::new("rabbitmq-4.2").unwrap())
        .unwrap();
    assert_eq!(s.pins.len(), 2);
}

#[test]
fn config_rejects_unknown_top_level_field() {
    let tmp = TempDir::new().unwrap();
    let path = write_config(&tmp, "config_version = 1\nblahblah = 5\n");
    let r = Config::load(&path);
    assert!(r.is_err());
}

#[test]
fn config_rejects_series_pin_to_unknown_project() {
    let tmp = TempDir::new().unwrap();
    let body = r#"
config_version = 1
[[project]]
name = "ra"
git_url = "/tmp/ra.git"
[[series]]
name = "broken"
pins = [{ project = "ghost", tag = "v1" }]
"#;
    let path = write_config(&tmp, body);
    let r = Config::load(&path);
    assert!(r.is_err());
}

#[test]
fn projects_missing_from_series_reports_unpinned_projects() {
    let tmp = TempDir::new().unwrap();
    let body = r#"
config_version = 1
[[project]]
name = "ra"
git_url = "/tmp/ra.git"
[[project]]
name = "osiris"
git_url = "/tmp/osiris.git"
[[project]]
name = "cowboy"
git_url = "/tmp/cowboy.git"
[[series]]
name = "partial"
pins = [{ project = "ra", tag = "v1" }]
"#;
    let path = write_config(&tmp, body);
    let cfg = Config::load(&path).unwrap();
    let series = cfg
        .series_by_name(&SeriesName::new("partial").unwrap())
        .unwrap();
    let missing = cfg.projects_missing_from_series(series);
    let names: Vec<&str> = missing.iter().map(|n| n.as_str()).collect();
    assert_eq!(names, vec!["cowboy", "osiris"]);
}

#[test]
fn projects_missing_from_series_empty_when_all_pinned() {
    let tmp = TempDir::new().unwrap();
    let body = r#"
config_version = 1
[[project]]
name = "ra"
git_url = "/tmp/ra.git"
[[series]]
name = "complete"
pins = [{ project = "ra", tag = "v1" }]
"#;
    let path = write_config(&tmp, body);
    let cfg = Config::load(&path).unwrap();
    let series = cfg
        .series_by_name(&SeriesName::new("complete").unwrap())
        .unwrap();
    assert!(cfg.projects_missing_from_series(series).is_empty());
}

#[test]
fn series_by_name_with_coverage_check_returns_same_series() {
    let tmp = TempDir::new().unwrap();
    let body = r#"
config_version = 1
[[project]]
name = "ra"
git_url = "/tmp/ra.git"
[[project]]
name = "osiris"
git_url = "/tmp/osiris.git"
[[series]]
name = "partial"
pins = [{ project = "ra", tag = "v1" }]
"#;
    let path = write_config(&tmp, body);
    let cfg = Config::load(&path).unwrap();
    let name = SeriesName::new("partial").unwrap();
    let direct = cfg.series_by_name(&name).unwrap();
    let checked = cfg.series_by_name_with_coverage_check(&name).unwrap();
    assert_eq!(direct.name, checked.name);
    assert_eq!(direct.pins.len(), checked.pins.len());
}
