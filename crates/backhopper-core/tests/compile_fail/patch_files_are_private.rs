use backhopper_core::compat::Patch;
use backhopper_core::compat::patch::PatchedFile;
use backhopper_core::compat::patch_state::Analyzed;

fn take_files(patch: &Patch<Analyzed>) -> Vec<PatchedFile> {
    patch.files
}

fn main() {}
