//! SPEC.md Section 7: the verifier must not depend on any crate under test.

const FORBIDDEN_PREFIXES: [&str; 4] = ["aien-", "spark-", "cortex-", "rad-"];

#[test]
fn verifier_has_no_dependency_on_code_under_test() {
    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("read Cargo.toml");
    let value: toml::Value = toml::from_str(&manifest).expect("parse Cargo.toml");

    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        let Some(deps) = value.get(section).and_then(|d| d.as_table()) else {
            continue;
        };
        for (name, spec) in deps {
            assert!(
                !FORBIDDEN_PREFIXES.iter().any(|p| name.starts_with(p)),
                "{name} is workspace code under test"
            );
            assert!(spec.get("path").is_none(), "{name} is a path dependency");
        }
    }
    assert!(
        value.get("target").is_none(),
        "target-specific dependencies must be reviewed for independence"
    );
}
