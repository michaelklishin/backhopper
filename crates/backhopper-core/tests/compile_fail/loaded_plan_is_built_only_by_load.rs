use backhopper_core::jigsaw::{JigsawPlan, LoadedPlan};

fn build_without_reading_the_store(plan: JigsawPlan) {
    let _ = LoadedPlan { plan };
}

fn main() {}
