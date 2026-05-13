use proptest::prelude::*;
use time::OffsetDateTime;

use backhopper_core::model::names::{
    Arity, CommitSha, FunctionName, ModuleName, ProjectName, TagName,
};
use backhopper_core::model::snapshot::{FunArity, Module, Snapshot, SnapshotHeader};
use backhopper_core::snapshot::{format, parser};

fn arb_lower_atom() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,5}".prop_map(|s| s)
}

fn arb_module() -> impl Strategy<Value = Module> {
    (
        arb_lower_atom(),
        prop::collection::vec((arb_lower_atom(), 0u8..=3), 0..4),
    )
        .prop_map(|(name, exports)| {
            let mut m = Module::new(ModuleName::new(name).unwrap());
            for (n, a) in exports {
                m.exports.push(FunArity {
                    name: FunctionName::new(n).unwrap(),
                    arity: Arity::new(a),
                });
            }
            m
        })
}

proptest! {
    #[test]
    fn snapshot_round_trip(
        project in "[a-z][a-z0-9_]{0,5}",
        tag in "v[0-9]{1,2}\\.[0-9]{1,2}\\.[0-9]{1,2}",
        modules in prop::collection::vec(arb_module(), 0..3)
    ) {
        let header = SnapshotHeader {
            project:      ProjectName::new(project).unwrap(),
            tag:          TagName::new(tag).unwrap(),
            branch:       None,
            commit:       CommitSha::new("0".repeat(40)).unwrap(),
            scanned_paths: vec!["src".into()],
            generated_by: "p".into(),
            generated_at: OffsetDateTime::from_unix_timestamp(0).unwrap(),
        };
        let snap = Snapshot::from_extracted(header, modules, vec![]).into_canonical();
        let text = format::to_string(&snap).unwrap();
        let back = parser::parse(&text).unwrap();
        prop_assert_eq!(snap, back);
    }
}
