use backhopper_xref_graph::{Building, Built, Functions, Mode, Modules, Phase};

#[test]
fn mode_names_are_stable_strings() {
    assert_eq!(Functions::NAME, "functions");
    assert_eq!(Modules::NAME, "modules");
}

#[test]
fn phase_names_are_stable_strings() {
    assert_eq!(Building::NAME, "building");
    assert_eq!(Built::NAME, "built");
}
