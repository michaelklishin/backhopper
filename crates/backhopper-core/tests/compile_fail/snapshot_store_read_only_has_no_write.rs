use backhopper_core::model::snapshot::{Snapshot, state};
use backhopper_core::store::{ReadOnly, SnapshotStore};

fn write_through_read_only(store: &SnapshotStore<ReadOnly>, snapshot: &Snapshot<state::Canonical>) {
    let _ = store.write(snapshot);
}

fn main() {}
