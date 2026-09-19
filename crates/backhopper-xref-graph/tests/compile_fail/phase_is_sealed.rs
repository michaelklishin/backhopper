use backhopper_xref_graph::Phase;

struct Foreign;

impl Phase for Foreign {
    const NAME: &'static str = "foreign";
}

fn main() {}
