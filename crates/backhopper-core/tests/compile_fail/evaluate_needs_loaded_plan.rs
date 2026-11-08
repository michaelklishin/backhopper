use backhopper_core::jigsaw::plugin_set::PluginSet;
use backhopper_core::jigsaw::references::ReferenceSet;
use backhopper_core::jigsaw::{JigsawPlan, evaluate};

fn evaluate_an_unchecked_plan(plan: &JigsawPlan, plugins: &PluginSet) {
    let _ = evaluate(plan, plugins, |_, _| Ok::<_, ()>(ReferenceSet::new()));
}

fn main() {}
