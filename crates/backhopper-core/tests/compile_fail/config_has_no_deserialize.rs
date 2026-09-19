use backhopper_core::config::Config;

fn main() {
    let _: Config = serde_json::from_str("{}").unwrap();
}
