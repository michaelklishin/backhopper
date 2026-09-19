use backhopper_cli::commands::self_snapshot::resolve_self_pin;
use backhopper_core::model::names::{GitRef, ProjectName};
use backhopper_core::model::pin::PinSpec;

fn main() {
    let spec = PinSpec::SelfRef {
        project: ProjectName::new("host").unwrap(),
        git_ref: GitRef::new("main").unwrap(),
        repo_dir_path: None,
    };
    let _ = resolve_self_pin(None, &spec);
}
