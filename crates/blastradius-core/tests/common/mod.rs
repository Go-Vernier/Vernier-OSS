#![allow(dead_code)]
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use blastradius::Service;
use blastradius::discover::DiscoveryResult;
use blastradius::fs::FileIndex;

pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures")
        .join(name)
        .canonicalize()
        .unwrap()
}

pub fn discover(name: &str) -> DiscoveryResult {
    let root = fixture(name);
    let index = FileIndex::build(&root);
    blastradius::discover::discover_services(&root, &index)
}

pub fn by_name(services: &[Service]) -> BTreeMap<String, Service> {
    services
        .iter()
        .map(|s| (s.name.clone(), s.clone()))
        .collect()
}
