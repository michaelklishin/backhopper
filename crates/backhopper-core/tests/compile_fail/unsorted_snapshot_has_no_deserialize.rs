use backhopper_core::model::snapshot::{Snapshot, state};

fn parse_unsorted(input: &str) -> Snapshot<state::Unsorted> {
    serde_json::from_str(input).unwrap()
}

fn main() {}
