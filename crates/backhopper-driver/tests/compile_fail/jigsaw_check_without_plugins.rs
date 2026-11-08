use backhopper_driver::{Backhopper, SubprocessBackend};

fn missing_plugins(driver: &Backhopper<SubprocessBackend>) {
    let builder = driver.jigsaw().check().plan_file("plan.json");
    let _ = builder.run();
}

fn main() {}
