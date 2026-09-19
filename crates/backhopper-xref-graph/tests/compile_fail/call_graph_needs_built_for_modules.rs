use backhopper_xref_graph::{Building, CallGraph, Functions};

fn missing_built(graph: &CallGraph<Functions, Building>) {
    let _ = graph.modules();
}

fn main() {}
