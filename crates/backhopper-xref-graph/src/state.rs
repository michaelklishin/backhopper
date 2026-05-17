//! Sealed type-state markers for [`CallGraph`](crate::graph::CallGraph).
//!
//! Mode picks which edge sets the graph stores; phase distinguishes the
//! under-construction graph from the queryable one. Both are sealed so
//! downstream crates cannot add new marker variants.

mod private {
    pub trait Sealed {}
}

pub trait Mode: private::Sealed {
    const NAME: &'static str;
}

pub trait Phase: private::Sealed {
    const NAME: &'static str;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Functions;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Modules;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Building;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Built;

impl private::Sealed for Functions {}
impl private::Sealed for Modules {}
impl private::Sealed for Building {}
impl private::Sealed for Built {}

impl Mode for Functions {
    const NAME: &'static str = "functions";
}
impl Mode for Modules {
    const NAME: &'static str = "modules";
}
impl Phase for Building {
    const NAME: &'static str = "building";
}
impl Phase for Built {
    const NAME: &'static str = "built";
}
