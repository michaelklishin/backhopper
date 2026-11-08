use backhopper_core::jigsaw::{HostRelease, JigsawPlan, SeriesReleases};

fn build_without_the_support_check(releases: SeriesReleases<HostRelease>) {
    let _ = JigsawPlan {
        releases,
    };
}

fn main() {}
