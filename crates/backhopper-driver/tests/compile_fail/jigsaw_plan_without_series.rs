use backhopper_driver::types::HostName;
use backhopper_driver::{Backhopper, SubprocessBackend};

fn missing_series(driver: &Backhopper<SubprocessBackend>, host: HostName) {
    let builder = driver.jigsaw().plan().host(host);
    let _ = builder.run();
}

fn main() {}
