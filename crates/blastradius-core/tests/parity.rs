//! The Rust engine must print what the TypeScript engine printed, for every
//! fixture. `root` and `repository` depend on the machine and are ignored.
mod common;

use common::fixture;
use pretty_assertions::assert_eq;
use serde_json::Value;
use std::fs;

fn normalise(mut v: Value) -> Value {
    let obj = v.as_object_mut().unwrap();
    obj.remove("root");
    obj.remove("repository");
    // Stage 2 added `mapping`; the baseline predates it.
    obj.remove("mapping");
    if let Some(services) = obj.get_mut("services").and_then(Value::as_array_mut) {
        services.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    }
    v
}

#[test]
fn every_fixture_matches_the_typescript_baseline() {
    let dir = fixture("..").join("expected/discovery");
    let mut checked = 0;
    let mut entries: Vec<_> = fs::read_dir(&dir).unwrap().flatten().collect();
    entries.sort_by_key(std::fs::DirEntry::path);
    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let name = path.file_stem().unwrap().to_str().unwrap();
        let expected: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let actual =
            serde_json::to_value(blastradius::analyze(&fixture(name)).unwrap().to_json()).unwrap();
        assert_eq!(normalise(actual), normalise(expected), "fixture {name}");
        checked += 1;
    }
    assert_eq!(checked, 10);
}
