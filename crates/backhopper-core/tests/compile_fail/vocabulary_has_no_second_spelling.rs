use backhopper_core::config::ProjectKind;

fn second_spelling(kind: ProjectKind) -> &'static str {
    kind.as_str()
}

fn main() {}
