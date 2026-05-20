// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::{ApplicationName, Arity, BehaviourName, FunctionName, Mfa, ModuleName};
use backhopper_xref_graph::Vertex;

fn mfa(m: &str, f: &str, a: u8) -> Mfa {
    Mfa::new(
        ModuleName::new(m.to_owned()).unwrap(),
        FunctionName::new(f.to_owned()).unwrap(),
        Arity::new(a),
    )
}

#[test]
fn function_vertex_extracts_mfa() {
    let v = Vertex::Function(mfa("foo", "bar", 2));
    assert_eq!(v.as_function().unwrap().function.as_str(), "bar");
    assert!(v.as_module().is_none());
}

#[test]
fn module_vertex_extracts_module_name() {
    let v = Vertex::Module(ModuleName::new("foo".to_owned()).unwrap());
    assert_eq!(v.as_module().unwrap().as_str(), "foo");
    assert!(v.as_function().is_none());
}

#[test]
fn application_vertex_extracts_app() {
    let v = Vertex::Application(ApplicationName::new("rabbit".to_owned()).unwrap());
    assert_eq!(v.as_application().unwrap().as_str(), "rabbit");
}

#[test]
fn behaviour_vertex_extracts_behaviour() {
    let v = Vertex::Behaviour(BehaviourName::new("gen_server".to_owned()).unwrap());
    assert_eq!(v.as_behaviour().unwrap().as_str(), "gen_server");
}

#[test]
fn display_function_vertex_matches_mfa() {
    let v = Vertex::Function(mfa("foo", "bar", 2));
    assert_eq!(v.to_string(), "foo:bar/2");
}

#[test]
fn display_module_vertex_is_module_name() {
    let v = Vertex::Module(ModuleName::new("rabbit_db_vhost".to_owned()).unwrap());
    assert_eq!(v.to_string(), "rabbit_db_vhost");
}

#[test]
fn display_application_vertex_carries_kind_prefix() {
    let v = Vertex::Application(ApplicationName::new("rabbit".to_owned()).unwrap());
    assert_eq!(v.to_string(), "app:rabbit");
}

#[test]
fn display_behaviour_vertex_carries_kind_prefix() {
    let v = Vertex::Behaviour(BehaviourName::new("gen_server".to_owned()).unwrap());
    assert_eq!(v.to_string(), "behaviour:gen_server");
}

#[test]
fn vertices_sort_function_before_module_alphabetically() {
    let a = Vertex::Function(mfa("zz", "a", 0));
    let b = Vertex::Module(ModuleName::new("aa".to_owned()).unwrap());
    let mut vs = [b.clone(), a.clone()];
    vs.sort();
    // Enum discriminant ordering: Function < Module < Application < Behaviour.
    assert_eq!(vs[0], a);
    assert_eq!(vs[1], b);
}

#[test]
fn vertex_serializes_with_kind_tag() {
    let v = Vertex::Module(ModuleName::new("rabbit".to_owned()).unwrap());
    let json = serde_json::to_string(&v).unwrap();
    assert!(json.contains("\"kind\":\"module\""));
    assert!(json.contains("\"rabbit\""));
}

#[test]
fn vertex_round_trips_through_json() {
    let v = Vertex::Function(mfa("foo", "bar", 2));
    let json = serde_json::to_string(&v).unwrap();
    let back: Vertex = serde_json::from_str(&json).unwrap();
    assert_eq!(back, v);
}

#[test]
fn vertex_hash_matches_eq() {
    use std::collections::HashSet;
    let mut s = HashSet::new();
    s.insert(Vertex::Function(mfa("a", "b", 0)));
    assert!(s.contains(&Vertex::Function(mfa("a", "b", 0))));
}
