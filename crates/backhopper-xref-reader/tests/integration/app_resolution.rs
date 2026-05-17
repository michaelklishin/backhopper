use std::path::PathBuf;

use backhopper_xref_reader::{ApplicationAssignment, ProjectLayout};

#[test]
fn rabbitmq_layout_resolves_deps_path() {
    let layout = ProjectLayout::rabbitmq_main();
    let p = PathBuf::from("/x/deps/rabbit/src/rabbit_db_vhost.erl");
    match layout.resolve(&p) {
        ApplicationAssignment::Assigned(app) => assert_eq!(app.as_str(), "rabbit"),
        other => panic!("expected Assigned, got {:?}", other),
    }
}

#[test]
fn rabbitmq_layout_resolves_apps_path() {
    let layout = ProjectLayout::rabbitmq_main();
    let p = PathBuf::from("/x/apps/my_plugin/src/my_plugin_sup.erl");
    match layout.resolve(&p) {
        ApplicationAssignment::Assigned(app) => assert_eq!(app.as_str(), "my_plugin"),
        other => panic!("expected Assigned, got {:?}", other),
    }
}

#[test]
fn unmatched_path_returns_no_layout_match() {
    let layout = ProjectLayout::rabbitmq_main();
    let p = PathBuf::from("/tmp/standalone.erl");
    assert!(matches!(
        layout.resolve(&p),
        ApplicationAssignment::NoLayoutMatch { .. }
    ));
}

#[test]
fn empty_layout_returns_no_layout_match() {
    let layout = ProjectLayout::new();
    let p = PathBuf::from("/x/deps/rabbit/src/foo.erl");
    assert!(matches!(
        layout.resolve(&p),
        ApplicationAssignment::NoLayoutMatch { .. }
    ));
}

#[test]
fn custom_prefix_resolves_application() {
    let mut layout = ProjectLayout::new();
    layout.add_prefix("plugins");
    let p = PathBuf::from("/x/plugins/widget/src/widget.erl");
    match layout.resolve(&p) {
        ApplicationAssignment::Assigned(app) => assert_eq!(app.as_str(), "widget"),
        other => panic!("expected Assigned, got {:?}", other),
    }
}

#[test]
fn assignment_assigned_returns_some_application_name() {
    use backhopper_core::ApplicationName;
    let assigned = ApplicationAssignment::Assigned(ApplicationName::new("foo".to_owned()).unwrap());
    assert_eq!(assigned.assigned().unwrap().as_str(), "foo");
}

#[test]
fn assignment_no_layout_match_returns_none() {
    let a = ApplicationAssignment::NoLayoutMatch {
        path: PathBuf::from("/x"),
    };
    assert!(a.assigned().is_none());
}

#[test]
fn assignment_ambiguous_returns_none() {
    let a = ApplicationAssignment::Ambiguous {
        candidates: Vec::new(),
    };
    assert!(a.assigned().is_none());
}

#[test]
fn multi_component_prefix_resolves_application() {
    let mut layout = ProjectLayout::new();
    layout.add_prefix("external/deps");
    let p = PathBuf::from("/x/external/deps/widget/src/widget.erl");
    match layout.resolve(&p) {
        ApplicationAssignment::Assigned(app) => assert_eq!(app.as_str(), "widget"),
        other => panic!("expected Assigned, got {:?}", other),
    }
}

#[test]
fn relative_path_resolves_application() {
    let layout = ProjectLayout::rabbitmq_main();
    let p = PathBuf::from("deps/rabbit_common/src/rabbit_misc.erl");
    match layout.resolve(&p) {
        ApplicationAssignment::Assigned(app) => assert_eq!(app.as_str(), "rabbit_common"),
        other => panic!("expected Assigned, got {:?}", other),
    }
}
