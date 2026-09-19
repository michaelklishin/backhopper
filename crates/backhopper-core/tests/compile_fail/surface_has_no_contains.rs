use std::collections::BTreeSet;

use backhopper_core::compat::source_attributes::Surface;

fn take_it(surface: &Surface<u32>) -> bool {
    surface.contains(&1)
}

fn main() {
    let _ = take_it(&Surface::new(BTreeSet::new(), None));
}
