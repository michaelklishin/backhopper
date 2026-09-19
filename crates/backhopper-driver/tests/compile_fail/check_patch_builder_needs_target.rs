use backhopper_driver::{Backhopper, SubprocessBackend};

fn missing_target(driver: &Backhopper<SubprocessBackend>) {
    let builder = driver.check().patch().patch_bytes(Vec::new());
    let _ = builder.run();
}

fn main() {}
