use backhopper_core::compat::test_suite::TestSuiteFile;
use backhopper_core::compat::test_suite::state::Parsed;

fn missing_modules_before_resolve(suite: &TestSuiteFile<Parsed>) {
    let _ = suite.missing_modules();
}

fn main() {}
