# Rust Engine: Stage 1 Port Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Rust workspace whose `blast-radius analyze <path> --json` prints exactly what the TypeScript engine prints for every fixture and every corpus repository, after which the TypeScript engine is removed.

**Architecture:** Two crates. `blastradius-core` is a library: file index, marked YAML loader, the four discovery strategies plus the fallback, the graph, `analyze`, and the terminal report. `blastradius-cli` is the `blast-radius` binary built on clap. One file walk feeds every strategy. The JSON contract is the TypeScript `AnalysisJSON`, field for field.

**Tech Stack:** Rust 1.98 (edition 2024), serde + serde_json, yaml-rust2 (event parser, own marked tree), toml, ignore + globset (walk and glob), regex, clap 4, owo-colors, pretty_assertions.

**Spec:** `docs/superpowers/specs/2026-09-06-rust-engine-stage2-design.md` (sections: Decision, Repository shape, JSON contract, Stage 1 port, Validation).

## Global Constraints

- Toolchain: stable Rust, currently 1.98.1, installed via Homebrew rustup. `cargo` lives at `/opt/homebrew/opt/rustup/bin`; export `PATH="/opt/homebrew/opt/rustup/bin:$PATH"` in every shell that runs cargo.
- `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` must pass before every commit.
- JSON field names are camelCase, exactly: `entryPoints`, `discoveredBy`, `packageName`. Optional fields are omitted when absent, never `null`. `root` on a Service is `null` for image-only services.
- Ordering: services code-first, then name case-insensitive, then byte order. `entryPoints` byte order. `attempted` in strategy order.
- Behaviour to preserve verbatim is listed in the spec's "Stage 1 port" section. When this plan and the TypeScript source under `src/` disagree, the TypeScript source wins until the parity test passes; then delete it.
- Fixture paths are `test/fixtures/<name>`; tests resolve them from `env!("CARGO_MANIFEST_DIR")` two levels up.
- No network access anywhere in the engine. `git remote get-url origin` is the only subprocess.
- Commit after every task with the message given in that task.

## File Structure

```
Cargo.toml                                   workspace, shared deps, lints
.gitignore                                   + target/
crates/blastradius-core/Cargo.toml
crates/blastradius-core/src/lib.rs           module list and re-exports
crates/blastradius-core/src/model.rs         Confidence, Evidence, Service, Edge, enums (serde)
crates/blastradius-core/src/fs.rs            FileIndex: one walk, rel paths, glob filters, read helpers
crates/blastradius-core/src/yaml.rs          marked YAML tree from yaml-rust2 events; anchors, aliases, merge keys
crates/blastradius-core/src/discover/mod.rs  discover_services: strategy order, fallback, sorting
crates/blastradius-core/src/discover/env.rs  dotenv parse, compose interpolation
crates/blastradius-core/src/discover/language.rs  manifests, language, entry points, packageName
crates/blastradius-core/src/discover/directories.rs  DirectoryIndex tiers, normalise, image_basename
crates/blastradius-core/src/discover/compose.rs
crates/blastradius-core/src/discover/kubernetes.rs
crates/blastradius-core/src/discover/monorepo.rs
crates/blastradius-core/src/discover/workspace.rs
crates/blastradius-core/src/graph.rs         BlastGraph: services by name, edges, inbound/outbound
crates/blastradius-core/src/analyze.rs       Analysis, analyze(), repository_name(), AnalysisJson
crates/blastradius-core/src/report.rs        format_repo_report(&Analysis, color)
crates/blastradius-core/tests/common/mod.rs  fixture(name) -> PathBuf, by_name()
crates/blastradius-core/tests/discovery.rs   port of test/discover.test.ts
crates/blastradius-core/tests/analyze.rs     port of test/analyze.test.ts + graph tests
crates/blastradius-core/tests/parity.rs      test/expected/discovery/*.json vs Rust output
crates/blastradius-core/tests/corpus.rs      test/expected/corpus/*.json vs Rust output; skips without corpus/
crates/blastradius-cli/Cargo.toml
crates/blastradius-cli/src/main.rs           clap: analyze [path] --json --no-color
crates/blastradius-cli/tests/cli.rs          runs the binary on fixtures
test/expected/discovery/<fixture>.json       TypeScript baseline (copied from scratchpad/baseline/fixtures)
test/expected/corpus/<repo>.json             strategy + code/infra service names per corpus repo
.github/workflows/ci.yml                     cargo jobs on ubuntu + macos
README.md                                    build/run instructions switch to cargo
package.json                                 trimmed to name reservation + corpus script
```

Baseline outputs captured from the TypeScript engine before any change live in
`/private/tmp/claude-501/-Users-soumyaranjanpanda-Dream-YC-Vernier-OSS/fd8a3efa-b35e-4c97-9072-c61d885be215/scratchpad/baseline/{fixtures,corpus}/<name>.{json,txt}`.
Task 14 copies them into `test/expected/`. If that directory is gone, regenerate with
`node dist/cli.js analyze <path> --json` before deleting the TypeScript code.

---

### Task 1: Workspace scaffold and the data model

**Files:**
- Create: `Cargo.toml`, `crates/blastradius-core/Cargo.toml`, `crates/blastradius-core/src/lib.rs`, `crates/blastradius-core/src/model.rs`, `crates/blastradius-cli/Cargo.toml`, `crates/blastradius-cli/src/main.rs`
- Modify: `.gitignore`

**Interfaces:**
- Produces: every type in `model.rs` below. All later tasks use them unchanged.

- [ ] **Step 1: Write the workspace manifests**

`Cargo.toml`:
```toml
[workspace]
resolver = "2"
members = ["crates/blastradius-core", "crates/blastradius-cli"]

[workspace.package]
version = "0.0.1"
edition = "2024"
license = "MIT"
repository = "https://github.com/Go-Vernier/Vernier-OSS"
rust-version = "1.85"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
yaml-rust2 = "0.12"
toml = "1"
ignore = "0.4"
globset = "0.4"
regex = "1"
indexmap = { version = "2", features = ["serde"] }
thiserror = "2"
anyhow = "1"
clap = { version = "4", features = ["derive"] }
owo-colors = "4"
pretty_assertions = "1"

[workspace.lints.clippy]
all = "warn"
pedantic = "warn"
module_name_repetitions = "allow"
missing_errors_doc = "allow"
missing_panics_doc = "allow"
must_use_candidate = "allow"

[profile.release]
lto = "thin"
codegen-units = 1
strip = true
```

`crates/blastradius-core/Cargo.toml`:
```toml
[package]
name = "blastradius-core"
description = "Which services can this change reach? Service discovery and dependency mapping for multi-service repositories."
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true

[lib]
name = "blastradius"
path = "src/lib.rs"

[dependencies]
serde.workspace = true
serde_json.workspace = true
yaml-rust2.workspace = true
toml.workspace = true
ignore.workspace = true
globset.workspace = true
regex.workspace = true
indexmap.workspace = true
thiserror.workspace = true
owo-colors.workspace = true

[dev-dependencies]
pretty_assertions.workspace = true

[lints]
workspace = true
```

`crates/blastradius-cli/Cargo.toml`:
```toml
[package]
name = "blastradius-cli"
description = "blast-radius command line"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true

[[bin]]
name = "blast-radius"
path = "src/main.rs"

[dependencies]
blastradius-core = { path = "../blastradius-core" }
clap.workspace = true
serde_json.workspace = true
anyhow.workspace = true

[dev-dependencies]
pretty_assertions.workspace = true

[lints]
workspace = true
```

Append `target/` to `.gitignore`.

- [ ] **Step 2: Write the failing model test**

`crates/blastradius-core/src/model.rs` starts with only the test module so it fails to compile:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn service_serialises_camel_case_and_omits_absent_fields() {
        let s = Service {
            name: "redis".into(),
            root: None,
            language: None,
            entry_points: vec![],
            role: ServiceRole::Infrastructure,
            discovered_by: ServiceSource::Strategy(DiscoveryStrategy::DockerCompose),
            evidence: Evidence { file: "docker-compose.yml".into(), line: Some(9), detail: None },
            image: Some("redis:7-alpine".into()),
            package_name: None,
        };
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "name": "redis", "root": null, "language": null, "entryPoints": [],
                "role": "infrastructure", "discoveredBy": "docker-compose",
                "evidence": { "file": "docker-compose.yml", "line": 9 },
                "image": "redis:7-alpine"
            })
        );
    }

    #[test]
    fn root_source_serialises_as_root_and_confidence_ranks() {
        assert_eq!(serde_json::to_value(ServiceSource::Root).unwrap(), serde_json::json!("root"));
        assert!(Confidence::Observed.rank() > Confidence::Static.rank());
        assert!(Confidence::Static.rank() > Confidence::Inferred.rank());
        assert!(Confidence::Inferred.rank() > Confidence::Uncertain.rank());
        assert_eq!(serde_json::to_value(EdgeType::Http).unwrap(), serde_json::json!("http"));
    }
}
```

- [ ] **Step 3: Run it to verify it fails**

Run: `cargo test -p blastradius-core model`
Expected: compile error, `Service` not found.

- [ ] **Step 4: Write the model**

Above the tests in `model.rs`:
```rust
//! The data every stage writes. Field names are the JSON contract.
use serde::{Deserialize, Serialize};

/// How sure we are that an edge exists. The weakest confidence on a path
/// decides the confidence of everything reached through it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence { Observed, Static, Inferred, Uncertain }

impl Confidence {
    pub fn rank(self) -> u8 {
        match self { Self::Observed => 3, Self::Static => 2, Self::Inferred => 1, Self::Uncertain => 0 }
    }
}

/// Where a fact came from. Every service and every edge carries one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    /// Path relative to the repository root, POSIX separators.
    pub file: String,
    /// 1-based line, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// Short note: the matched key, the image, the URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiscoveryStrategy { DockerCompose, Kubernetes, Monorepo, Workspace }

impl DiscoveryStrategy {
    pub const ALL: [Self; 4] = [Self::DockerCompose, Self::Kubernetes, Self::Monorepo, Self::Workspace];
}

/// "root" is the honest fallback: the repository itself is one service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ServiceSource {
    Strategy(DiscoveryStrategy),
    #[serde(with = "root_source")]
    Root,
}

mod root_source {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};
    pub fn serialize<S: Serializer>(s: S) -> Result<S::Ok, S::Error> { s.serialize_str("root") }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<(), D::Error> {
        match String::deserialize(d)?.as_str() { "root" => Ok(()), other => Err(D::Error::custom(format!("expected \"root\", got {other}"))) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceRole { Code, Infrastructure }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Service {
    pub name: String,
    /// Directory relative to the repository root. None for image-only services.
    pub root: Option<String>,
    pub language: Option<String>,
    /// Files that start the service, relative to its root. Best effort.
    pub entry_points: Vec<String>,
    pub role: ServiceRole,
    pub discovered_by: ServiceSource,
    pub evidence: Evidence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EdgeType { Http, Event, Grpc, Database, Import }

/// `source` depends on `target`: source calls, publishes to, imports from,
/// or shares a database with target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub source: String,
    pub target: String,
    #[serde(rename = "type")]
    pub edge_type: EdgeType,
    pub confidence: Confidence,
    pub evidence: Vec<Evidence>,
}
```

Note on `ServiceSource`: serde `untagged` with a unit variant needs the `with` module so `Root` prints as the string `"root"`. If the compiler rejects `#[serde(with)]` on a unit variant in an untagged enum, replace the enum with a `String` newtype that validates the five values; the JSON must read `"docker-compose" | "kubernetes" | "monorepo" | "workspace" | "root"`.

`lib.rs`:
```rust
//! Blast Radius: which services can this change reach?
pub mod model;
pub use model::*;
```

`crates/blastradius-cli/src/main.rs` for now:
```rust
fn main() { println!("blast-radius"); }
```

- [ ] **Step 5: Run tests, fmt, clippy**

Run: `cargo test -p blastradius-core && cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: 2 passed, no warnings.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock .gitignore crates
git commit -m "feat(rust): workspace scaffold and the data model"
```

---

### Task 2: File index

**Files:**
- Create: `crates/blastradius-core/src/fs.rs`
- Modify: `crates/blastradius-core/src/lib.rs` (add `pub mod fs;`)
- Test: inline unit tests using `test/fixtures/compose-app`

**Interfaces:**
- Produces:
  ```rust
  pub struct FileIndex { root: PathBuf, files: Vec<String>, dirs: Vec<String> }
  impl FileIndex {
      pub fn build(root: &Path) -> Self;                       // one walk; files and dirs, repo-relative POSIX, sorted
      pub fn root(&self) -> &Path;
      pub fn files(&self) -> &[String];
      pub fn dirs(&self) -> &[String];                         // excludes "."
      pub fn files_matching(&self, include: &[&str], exclude: &[&str]) -> Vec<String>;  // glob patterns
      pub fn dirs_matching(&self, include: &[&str], exclude: &[&str]) -> Vec<String>;
      pub fn dirs_up_to_depth(&self, depth: usize) -> Vec<String>; // depth 1 = top-level children
  }
  pub fn is_dir(p: &Path) -> bool; pub fn is_file(p: &Path) -> bool;
  pub fn read_text(p: &Path) -> Option<String>;              // None on any error, invalid UTF-8 lossily decoded
  pub fn read_json(p: &Path) -> Option<serde_json::Value>;
  pub fn rel(root: &Path, abs: &Path) -> Option<String>;      // "." for root itself, None outside
  pub fn posix(p: &Path) -> String;
  pub const IGNORE_DIRS: &[&str];
  ```

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures").join(name).canonicalize().unwrap()
    }

    #[test]
    fn indexes_files_and_dirs_relative_posix_sorted() {
        let ix = FileIndex::build(&fixture("compose-app"));
        assert!(ix.files().contains(&"docker-compose.yml".to_string()));
        assert!(ix.files().contains(&"services/checkout/src/index.ts".to_string()));
        assert_eq!(ix.dirs_up_to_depth(1), vec!["services"]);
        assert!(ix.dirs().contains(&"services/checkout/src".to_string()));
        let mut sorted = ix.files().to_vec(); sorted.sort();
        assert_eq!(&sorted, ix.files());
    }

    #[test]
    fn glob_filters_match_whole_relative_paths() {
        let ix = FileIndex::build(&fixture("compose-app"));
        assert_eq!(ix.files_matching(&["**/docker-compose*.{yml,yaml}", "**/compose.{yml,yaml}"], &[]), vec!["docker-compose.yml"]);
        assert_eq!(ix.dirs_matching(&["services/*"], &["services/legacy"]), vec!["services/checkout", "services/orders"]);
        assert_eq!(ix.files_matching(&["**/*.{yml,yaml}"], &["**/docker-compose*.{yml,yaml}"]), Vec::<String>::new());
    }

    #[test]
    fn rel_and_reads() {
        let root = fixture("compose-app");
        assert_eq!(rel(&root, &root), Some(".".into()));
        assert_eq!(rel(&root, &root.join("services/orders")), Some("services/orders".into()));
        assert_eq!(rel(&root, root.parent().unwrap()), None);
        assert!(is_dir(&root) && !is_file(&root));
        assert!(read_text(&root.join("docker-compose.yml")).unwrap().contains("services:"));
        assert_eq!(read_text(&root.join("missing")), None);
        assert_eq!(read_json(&root.join("services/checkout/package.json")).unwrap()["name"], "@acme/checkout");
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p blastradius-core fs::`
Expected: compile error.

- [ ] **Step 3: Implement**

Rules:
- Walk with `ignore::WalkBuilder::new(root)`, `.hidden(true)` (default, skips dotfiles and dotdirs), `.git_ignore(true)`, `.follow_links(false)`, `.filter_entry(|e| !IGNORE_DIRS.contains(&e.file_name().to_string_lossy().as_ref()) || e.depth() == 0)`.
- `IGNORE_DIRS = ["node_modules", ".git", "vendor", "dist", "build", "target", ".next", "__pycache__", ".venv", "venv", "bin", "obj"]`.
- Skip the root entry itself for `dirs`. Store repo-relative POSIX paths via `rel`. Sort both vectors.
- Globs: `globset::GlobBuilder::new(p).literal_separator(true).build()`. Build one `GlobSet` for include and one for exclude; keep a path if include matches and exclude does not. For `files_matching` match against files; for `dirs_matching` against dirs. Note `**/x` must also match `x` at the root: globset does this when `literal_separator(true)`; if a test shows it does not, also test the pattern with a leading `**/` stripped.
- `dirs_up_to_depth(n)`: dirs whose segment count (`split('/')`) is `<= n`, in the sorted order of `dirs()`.
- `read_text`: `std::fs::read` then `String::from_utf8_lossy` into owned; `None` on error.
- `rel`: `abs.strip_prefix(root)`; empty -> "."; components joined with '/'.

- [ ] **Step 4: Run tests, fmt, clippy**

Run: `cargo test -p blastradius-core fs:: && cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/blastradius-core/src
git commit -m "feat(rust): file index with one walk and glob filters"
```

---

### Task 3: Marked YAML loader

**Files:**
- Create: `crates/blastradius-core/src/yaml.rs`
- Modify: `lib.rs` (add `pub mod yaml;`)

**Interfaces:**
- Produces:
  ```rust
  #[derive(Debug, Clone, PartialEq)]
  pub struct Node { pub line: u32, pub kind: Kind }        // line is 1-based
  #[derive(Debug, Clone, PartialEq)]
  pub enum Kind { Null, Bool(bool), Int(i64), Float(f64), Str(String), Seq(Vec<Node>), Map(Vec<(Node, Node)>) }
  impl Node {
      pub fn get(&self, key: &str) -> Option<&Node>;          // Map lookup by string key, first match
      pub fn as_str(&self) -> Option<&str>;                   // Str only. Ints/bools are not coerced
      pub fn as_scalar_string(&self) -> Option<String>;      // Str, Int, Float, Bool rendered as text (compose `image: 3.7` cases)
      pub fn entries(&self) -> &[(Node, Node)];              // empty slice when not a Map
      pub fn items(&self) -> &[Node];                        // empty slice when not a Seq
      pub fn is_map(&self) -> bool;
  }
  /// Every document that parses. A document that fails is skipped; parsing stops at the first error, so later documents in the same file are lost.
  pub fn parse_documents(text: &str) -> Vec<Node>;
  ```

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn keys_carry_their_line() {
        let docs = parse_documents("services:\n  checkout:\n    build: ./c\n  orders:\n    image: x\n");
        let services = docs[0].get("services").unwrap();
        let names: Vec<(&str, u32)> = services.entries().iter().map(|(k, _)| (k.as_str().unwrap(), k.line)).collect();
        assert_eq!(names, vec![("checkout", 2), ("orders", 4)]);
        assert_eq!(docs[0].line, 1);
    }

    #[test]
    fn multiple_documents_and_document_line() {
        let docs = parse_documents("---\napiVersion: v1\nkind: Service\n---\n\nkind: Deployment\n");
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0].get("kind").unwrap().as_str(), Some("Service"));
        assert_eq!(docs[0].line, 2);
        assert_eq!(docs[1].line, 6);
    }

    #[test]
    fn anchors_aliases_and_merge_keys_resolve() {
        let text = "x-env: &env\n  A: '1'\n  B: '2'\nservices:\n  a:\n    environment:\n      <<: *env\n      C: '3'\n  b:\n    environment: *env\n";
        let docs = parse_documents(text);
        let a = docs[0].get("services").unwrap().get("a").unwrap().get("environment").unwrap();
        let keys: Vec<&str> = a.entries().iter().map(|(k, _)| k.as_str().unwrap()).collect();
        assert_eq!(keys, vec!["A", "B", "C"]);
        let b = docs[0].get("services").unwrap().get("b").unwrap().get("environment").unwrap();
        assert_eq!(b.get("B").unwrap().as_str(), Some("2"));
    }

    #[test]
    fn broken_yaml_yields_no_documents_and_scalars_render() {
        assert!(parse_documents("a: [unclosed").is_empty());
        let docs = parse_documents("image: 3.7\nport: 8080\nflag: true\n");
        assert_eq!(docs[0].get("image").unwrap().as_scalar_string(), Some("3.7".into()));
        assert_eq!(docs[0].get("port").unwrap().as_str(), None);
        assert_eq!(docs[0].get("port").unwrap().as_scalar_string(), Some("8080".into()));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p blastradius-core yaml::`

- [ ] **Step 3: Implement**

Use `yaml_rust2::parser::{Parser, Event, MarkedEventReceiver}` and `yaml_rust2::scanner::Marker`. Build a receiver holding a stack of partially built containers:

```rust
enum Frame { Seq { line: u32, items: Vec<Node>, anchor: usize }, Map { line: u32, entries: Vec<(Node, Node)>, pending_key: Option<Node>, anchor: usize } }
struct Loader { docs: Vec<Node>, stack: Vec<Frame>, anchors: HashMap<usize, Node>, doc_root: Option<Node> }
```

- `Event::Scalar(value, style, anchor_id, tag)`: make a `Node`. Plain style with tag `None`: resolve `~`/`null`/empty -> Null, `true`/`false` -> Bool, integer parse -> Int, float parse -> Float, else Str. Quoted styles are always `Str`. Store in `anchors` when `anchor_id != 0`. Push into the top frame (as pending key or value).
- `Event::SequenceStart(anchor_id, _)` / `MappingStart(anchor_id, _)`: push a frame with `line = marker.line()`.
- `SequenceEnd` / `MappingEnd`: pop, build Node, register anchor, push into parent or set `doc_root`.
- `Event::Alias(anchor_id)`: clone from `anchors` (or Null if unknown) and push.
- When pushing a value into a Map whose pending key is the plain scalar `<<`: if the value is a Map, append its entries that are not already present (existing keys win); if it is a Seq of Maps, do the same for each in order. Do not store the `<<` key itself.
- `DocumentEnd`: push `doc_root.take()` into `docs` if present.
- `Marker::line()` in yaml-rust2 is 1-based; confirm with the first test and adjust with `+1` if it is not.
- `parse_documents`: `Parser::new_from_str(text).load(&mut loader, true)`; on `Err`, return the documents completed so far (`loader.docs`).
- `as_scalar_string`: `Str` -> clone; `Int`/`Float`/`Bool` -> `to_string()`; but a `Float` that was written as `3.7` must render as `3.7` (Rust `f64::to_string` gives `3.7`, fine; `1.0` gives `1`, which is acceptable for image tags in practice; note it).

- [ ] **Step 4: Run tests, fmt, clippy**

Run: `cargo test -p blastradius-core yaml:: && cargo fmt --check && cargo clippy --all-targets -- -D warnings`

- [ ] **Step 5: Commit**

```bash
git add crates/blastradius-core/src
git commit -m "feat(rust): marked YAML loader with anchors and merge keys"
```

---

### Task 4: Compose environment

**Files:**
- Create: `crates/blastradius-core/src/discover/mod.rs` (module declarations only for now), `crates/blastradius-core/src/discover/env.rs`
- Modify: `lib.rs` (add `pub mod discover;`)

**Interfaces:**
- Produces:
  ```rust
  pub type Env = IndexMap<String, String>;
  pub fn parse_dotenv(text: &str) -> Env;
  pub fn interpolate(value: &str, env: &Env) -> String;   // ${VAR}, ${VAR:-d}, ${VAR-d}, $VAR; unknown left in place
  pub fn has_unresolved(value: &str) -> bool;
  pub fn load_compose_env(compose_dir: &Path) -> Env;    // .env.example then .env; later wins
  ```

- [ ] **Step 1: Write the failing tests** (port of the "compose environment" describe block)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn parses_dotenv() {
        let env = parse_dotenv("# comment\nPLAIN=value\nexport EXPORTED=yes\nQUOTED=\"a # not a comment\"\nSINGLE='x'\nTRAILING=abc # comment\nEMPTY=\nnot a valid line\n");
        let got: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        assert_eq!(got, vec![("PLAIN", "value"), ("EXPORTED", "yes"), ("QUOTED", "a # not a comment"), ("SINGLE", "x"), ("TRAILING", "abc"), ("EMPTY", "")]);
    }

    #[test]
    fn interpolates_and_leaves_unknown_visible() {
        let mut env = Env::new();
        env.insert("IMAGE".into(), "ghcr.io/x".into());
        env.insert("EMPTY".into(), String::new());
        assert_eq!(interpolate("${IMAGE}:${TAG:-latest}", &env), "ghcr.io/x:latest");
        assert_eq!(interpolate("${EMPTY:-fallback}", &env), "fallback");
        assert_eq!(interpolate("${TAG-dash}", &env), "dash");
        assert_eq!(interpolate("$IMAGE/svc", &env), "ghcr.io/x/svc");
        assert_eq!(interpolate("${MISSING}", &env), "${MISSING}");
        assert!(has_unresolved("${MISSING}"));
        assert!(!has_unresolved("plain"));
    }

    #[test]
    fn loads_env_beside_compose_file() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures/compose-env-app");
        let env = load_compose_env(&dir);
        assert!(env.contains_key("IMAGE_NAME") || !env.is_empty());
    }
}
```
Check `test/fixtures/compose-env-app/.env` for the real key names and assert on one of them exactly.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p blastradius-core env::`

- [ ] **Step 3: Implement**

- Dotenv line regex: `^(?:export\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*)$` after trimming; skip blank and `#`. Value: if wrapped in matching double or single quotes (len >= 2) strip them; else strip a trailing `\s+#.*` and trim.
- Interpolation regex: `\$\{([A-Za-z_][A-Za-z0-9_]*)(?::?-([^}]*))?\}|\$([A-Za-z_][A-Za-z0-9_]*)`. Use `Regex::replace_all` with a closure: key from group 1 or 3; if env has a non-empty value use it; else if group 2 matched use it; else leave the whole match.
- `has_unresolved`: regex `\$\{?[A-Za-z_]`.
- Compile regexes once with `std::sync::LazyLock`.

- [ ] **Step 4: Run tests, fmt, clippy**

- [ ] **Step 5: Commit**

```bash
git add crates/blastradius-core/src
git commit -m "feat(rust): dotenv parsing and compose interpolation"
```

---

### Task 5: Language and manifest detection

**Files:**
- Create: `crates/blastradius-core/src/discover/language.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct Manifest { pub file: String, pub language: Option<String> }
  pub struct DirectoryDescription { pub manifest: Option<Manifest>, pub language: Option<String>, pub entry_points: Vec<String>, pub package_name: Option<String> }
  pub fn detect_manifest(dir_abs: &Path) -> Option<Manifest>;
  pub fn describe_directory(dir_abs: &Path) -> DirectoryDescription;
  pub fn detect_entry_points(dir_abs: &Path, language: Option<&str>) -> Vec<String>;
  ```

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    fn fixture(p: &str) -> PathBuf { Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures").join(p) }

    #[test]
    fn manifest_order_and_typescript_upgrade() {
        let m = detect_manifest(&fixture("compose-app/services/checkout")).unwrap();
        assert_eq!((m.file.as_str(), m.language.as_deref()), ("package.json", Some("typescript")));
        let m = detect_manifest(&fixture("compose-app/services/orders")).unwrap();
        assert_eq!((m.file.as_str(), m.language.as_deref()), ("go.mod", Some("go")));
        let m = detect_manifest(&fixture("k8s-app/src/cartservice")).unwrap();
        assert_eq!((m.file.as_str(), m.language.as_deref()), ("cartservice.csproj", Some("csharp")));
        assert!(detect_manifest(&fixture("monorepo-app/services/docs-only")).is_none());
    }

    #[test]
    fn describes_entry_points_and_package_name() {
        let d = describe_directory(&fixture("compose-app/services/checkout"));
        assert_eq!(d.package_name.as_deref(), Some("@acme/checkout"));
        assert!(d.entry_points.contains(&"src/index.ts".to_string()));
        let d = describe_directory(&fixture("compose-app/services/orders"));
        assert_eq!(d.entry_points, vec!["main.go"]);
        let d = describe_directory(&fixture("monorepo-app/services/worker"));
        assert_eq!((d.language.as_deref(), d.entry_points), (Some("python"), vec!["main.py".to_string()]));
    }
}
```

- [ ] **Step 2: Run to verify failure**

- [ ] **Step 3: Implement**

Port `src/discover/language.ts` exactly:
- `MANIFESTS` ordered list: package.json/javascript, go.mod/go, pom.xml/java, build.gradle/java, build.gradle.kts/kotlin, requirements.txt/python, pyproject.toml/python, Pipfile/python, Cargo.toml/rust, Gemfile/ruby, composer.json/php, mix.exs/elixir. `package.json` with a sibling `tsconfig.json` -> typescript.
- Then the first `*.csproj` or `*.fsproj` in the directory (read_dir, filter by extension, sort by name, take first): csharp or fsharp.
- Then `Dockerfile` -> `Manifest { file: "Dockerfile", language: None }`.
- `describe_directory`: manifest, language, entry points, and `package_name` from `package.json` `name` when the manifest is package.json.
- `detect_entry_points`: candidates by language as in the TypeScript source (javascript/typescript: package.json `main`, `bin` string or object values, then the fixed list `src/index.ts, src/main.ts, src/server.ts, src/app.ts, src/index.js, src/main.js, src/server.js, src/app.js, index.js, server.js, app.js, main.js`; go: `main.go` plus every `cmd/*/main.go`; python: `main.py, app.py, manage.py, server.py, __main__.py, wsgi.py, asgi.py`; rust: `src/main.rs`; ruby: `config.ru, app.rb`; php: `public/index.php, index.php`). Normalise candidates: strip leading `./`, POSIX separators. Keep those that are files, deduplicate, sort.

- [ ] **Step 4: Run tests, fmt, clippy**

- [ ] **Step 5: Commit**

```bash
git add crates/blastradius-core/src
git commit -m "feat(rust): manifest, language and entry point detection"
```

---

### Task 6: Directory index and name normalisation

**Files:**
- Create: `crates/blastradius-core/src/discover/directories.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct DirectoryIndex<'a> { root: &'a Path, dirs: Vec<String> }   // dirs up to depth 4, sorted by depth then name
  impl<'a> DirectoryIndex<'a> {
      pub fn build(index: &'a FileIndex) -> Self;
      pub fn matching(&self, names: &[Option<&str>]) -> Option<String>;   // first tiered match that has a manifest
  }
  pub fn normalise(s: &str) -> String;                 // lowercase, drop -_. , strip trailing service|svc|api|server|deployment|deploy
  pub fn image_basename(image: Option<&str>) -> Option<String>;
  ```

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::FileIndex;
    use pretty_assertions::assert_eq;
    fn fixture(p: &str) -> PathBuf { Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures").join(p).canonicalize().unwrap() }

    #[test]
    fn normalises_serviceish_suffixes() {
        assert_eq!(normalise("checkout-api"), "checkout");
        assert_eq!(normalise("CheckoutService"), "checkout");
        assert_eq!(normalise("checkout_svc"), "checkout");
        assert_eq!(normalise("redis-cart"), "rediscart");
        assert_eq!(normalise("Basket.API"), "basket");
    }

    #[test]
    fn image_basename_strips_registry_tag_digest() {
        assert_eq!(image_basename(Some("gcr.io/google-samples/microservices-demo/cartservice:v0.10.0")).as_deref(), Some("cartservice"));
        assert_eq!(image_basename(Some("redis:7-alpine")).as_deref(), Some("redis"));
        assert_eq!(image_basename(Some("ghcr.io/acme/api@sha256:abcdef")).as_deref(), Some("api"));
        assert_eq!(image_basename(None), None);
    }

    #[test]
    fn matches_by_exact_then_normalised_then_suffix_and_needs_a_manifest() {
        let ix = FileIndex::build(&fixture("compose-images-app"));
        let d = DirectoryIndex::build(&ix);
        assert_eq!(d.matching(&[Some("customers-service"), None]).as_deref(), Some("spring-petclinic-customers-service"));
        assert_eq!(d.matching(&[Some("tracing-server"), Some("zipkin")]), None);
        let ix = FileIndex::build(&fixture("k8s-app"));
        let d = DirectoryIndex::build(&ix);
        assert_eq!(d.matching(&[Some("cartservice"), None]).as_deref(), Some("src/cartservice"));
        assert_eq!(d.matching(&[None, Some("frontend")]).as_deref(), Some("src/frontend"));
    }
}
```

- [ ] **Step 2: Run to verify failure**

- [ ] **Step 3: Implement**

Port `src/discover/directories.ts`:
- `build`: `index.dirs_up_to_depth(4)` sorted by `(segment count, name)`.
- `matching`: dedupe the `Some` names in order. Three tier vectors. For each dir in order and each wanted name: `base = basename.to_lowercase()`, `lower = name.to_lowercase()`; exact `base == lower` -> tier 0; else `normalise(name) != "" && normalise(base) == normalise(name)` -> tier 1; else `lower.len() >= 4 && base.ends_with(&lower)` and the character right before the suffix is one of `- _ .` -> tier 2. Return the first candidate across tiers 0, 1, 2 for which `language::detect_manifest(root.join(dir))` is `Some`.
- `normalise`: lowercase; remove `-`, `_`, `.`; strip one trailing suffix from `service|svc|api|server|deployment|deploy` (regex `(service|svc|api|server|deployment|deploy)$`, single replacement).
- `image_basename`: last `/` segment, then split at `@`, then at `:`; empty -> None.

- [ ] **Step 4: Run tests, fmt, clippy**

- [ ] **Step 5: Commit**

```bash
git add crates/blastradius-core/src
git commit -m "feat(rust): directory index with tiered name matching"
```

---

### Task 7: docker-compose strategy and the discovery runner

**Files:**
- Create: `crates/blastradius-core/src/discover/compose.rs`, `crates/blastradius-core/tests/common/mod.rs`, `crates/blastradius-core/tests/discovery.rs`
- Modify: `crates/blastradius-core/src/discover/mod.rs`

**Interfaces:**
- Produces:
  ```rust
  // discover/mod.rs
  pub struct DiscoveryAttempt { pub strategy: DiscoveryStrategy, pub services: usize }   // serde camelCase
  pub struct DiscoveryResult { pub services: Vec<Service>, pub strategy: Option<DiscoveryStrategy>, pub attempted: Vec<DiscoveryAttempt> }
  pub fn discover_services(root: &Path, index: &FileIndex) -> DiscoveryResult;
  pub fn sort_services(services: &mut [Service]);       // code first, then name case-insensitive, then bytes
  pub(crate) fn code_count(services: &[Service]) -> usize;
  // compose.rs
  pub const COMPOSE_PATTERNS: [&str; 2] = ["**/docker-compose*.{yml,yaml}", "**/compose.{yml,yaml}"];
  pub fn discover_from_compose(root: &Path, index: &FileIndex) -> Vec<Service>;
  ```
- The runner for now has only the compose strategy wired; Tasks 8 to 10 add the rest to `STRATEGIES`.

- [ ] **Step 1: Write the shared test helper and the compose tests**

`tests/common/mod.rs`:
```rust
use blastradius::{Service, discover::DiscoveryResult, fs::FileIndex};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures").join(name).canonicalize().unwrap()
}
pub fn discover(name: &str) -> DiscoveryResult {
    let root = fixture(name);
    let index = FileIndex::build(&root);
    blastradius::discover::discover_services(&root, &index)
}
pub fn by_name(services: &[Service]) -> BTreeMap<String, Service> {
    services.iter().map(|s| (s.name.clone(), s.clone())).collect()
}
```

`tests/discovery.rs`, compose part:
```rust
mod common;
use blastradius::*;
use common::{by_name, discover};
use pretty_assertions::assert_eq;

#[test]
fn compose_reads_services_build_contexts_images_and_lines() {
    let r = discover("compose-app");
    assert_eq!(r.strategy, Some(DiscoveryStrategy::DockerCompose));
    let s = by_name(&r.services);
    assert_eq!(s.keys().cloned().collect::<Vec<_>>(), vec!["checkout", "orders", "redis"]);
    let c = &s["checkout"];
    assert_eq!(c.root.as_deref(), Some("services/checkout"));
    assert_eq!(c.language.as_deref(), Some("typescript"));
    assert_eq!(c.role, ServiceRole::Code);
    assert_eq!(c.package_name.as_deref(), Some("@acme/checkout"));
    assert_eq!(c.evidence, Evidence { file: "docker-compose.yml".into(), line: Some(2), detail: Some("build: ./services/checkout".into()) });
    assert!(c.entry_points.contains(&"src/index.ts".to_string()));
    let o = &s["orders"];
    assert_eq!((o.root.as_deref(), o.language.as_deref(), o.role), (Some("services/orders"), Some("go"), ServiceRole::Code));
    assert_eq!(o.entry_points, vec!["main.go"]);
    assert_eq!(o.evidence.line, Some(5));
    let redis = &s["redis"];
    assert_eq!((redis.root.as_deref(), redis.role, redis.image.as_deref()), (None, ServiceRole::Infrastructure, Some("redis:7-alpine")));
    assert_eq!((redis.evidence.file.as_str(), redis.evidence.line), ("docker-compose.yml", Some(9)));
}

#[test]
fn compose_wins_over_monorepo_so_undeclared_directory_is_not_a_service() {
    let r = discover("compose-app");
    assert!(!r.services.iter().any(|s| s.name == "legacy"));
    assert_eq!(r.attempted.len(), 1);
    assert_eq!((r.attempted[0].strategy, r.attempted[0].services), (DiscoveryStrategy::DockerCompose, 2));
}

#[test]
fn compose_matches_image_only_services_to_directories() {
    let r = discover("compose-images-app");
    assert_eq!(r.strategy, Some(DiscoveryStrategy::DockerCompose));
    let s = by_name(&r.services);
    let c = &s["customers-service"];
    assert_eq!(c.root.as_deref(), Some("spring-petclinic-customers-service"));
    assert_eq!(c.language.as_deref(), Some("java"));
    assert_eq!(c.image.as_deref(), Some("springcommunity/spring-petclinic-customers-service:3.2.0"));
    assert_eq!(c.evidence.detail.as_deref(), Some("image: springcommunity/spring-petclinic-customers-service:3.2.0"));
    assert_eq!(s["vets-service"].root.as_deref(), Some("spring-petclinic-vets-service"));
    assert_eq!(s["config-server"].root.as_deref(), Some("spring-petclinic-config-server"));
    let t = &s["tracing-server"];
    assert_eq!((t.root.as_deref(), t.role, t.image.as_deref()), (None, ServiceRole::Infrastructure, Some("openzipkin/zipkin")));
}

#[test]
fn compose_uses_dockerfile_directory_when_context_is_root() {
    let r = discover("compose-rootctx-app");
    let s = by_name(&r.services);
    let a = &s["accounting"];
    assert_eq!((a.root.as_deref(), a.language.as_deref(), a.role), (Some("src/accounting"), Some("go"), ServiceRole::Code));
    assert_eq!(a.evidence.detail.as_deref(), Some("build: ./, dockerfile: ./src/accounting/Dockerfile"));
    let f = &s["frontend"];
    assert_eq!((f.root.as_deref(), f.language.as_deref(), f.package_name.as_deref()), (Some("src/frontend"), Some("javascript"), Some("frontend")));
    assert_eq!((s["kafka"].root.as_deref(), s["kafka"].role), (None, ServiceRole::Infrastructure));
}

#[test]
fn compose_resolves_env_references() {
    let r = discover("compose-env-app");
    let s = by_name(&r.services);
    let ad = &s["ad"];
    assert_eq!((ad.root.as_deref(), ad.language.as_deref(), ad.image.as_deref()), (Some("src/ad"), Some("java"), Some("ghcr.io/demo:1.0-ad")));
    assert_eq!(ad.evidence.detail.as_deref(), Some("build: ./, dockerfile: ./src/ad/Dockerfile"));
    let cart = &s["cart"];
    assert_eq!((cart.root.as_deref(), cart.language.as_deref()), (Some("src/cart"), Some("go")));
    assert_eq!(cart.evidence.detail.as_deref(), Some("build: ./, matched directory src/cart"));
    let db = &s["db"];
    assert_eq!((db.root.as_deref(), db.role, db.image.as_deref()), (None, ServiceRole::Infrastructure, Some("${POSTGRES_IMAGE}")));
}

#[test]
fn compose_keeps_deploy_only_images() {
    let r = discover("deploy-only-app");
    assert_eq!(r.strategy, None);
    assert_eq!(r.services.len(), 3);
    assert!(r.services.iter().all(|s| s.role == ServiceRole::Infrastructure));
    assert_eq!((r.attempted[0].strategy, r.attempted[0].services), (DiscoveryStrategy::DockerCompose, 0));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p blastradius-core --test discovery`
Expected: compile error, `discover_services` missing.

- [ ] **Step 3: Implement the runner**

`discover/mod.rs`:
```rust
pub mod compose; pub mod directories; pub mod env; pub mod language;
// kubernetes, monorepo, workspace added in Tasks 8-10

type Strategy = fn(&Path, &FileIndex) -> Vec<Service>;
const STRATEGIES: &[(DiscoveryStrategy, Strategy)] = &[
    (DiscoveryStrategy::DockerCompose, compose::discover_from_compose),
];

pub fn discover_services(root: &Path, index: &FileIndex) -> DiscoveryResult {
    let mut attempted = Vec::new();
    let mut fallback: Vec<Service> = Vec::new();
    for (strategy, run) in STRATEGIES {
        let mut services = run(root, index);
        let code = code_count(&services);
        attempted.push(DiscoveryAttempt { strategy: *strategy, services: code });
        if code > 1 { sort_services(&mut services); return DiscoveryResult { services, strategy: Some(*strategy), attempted }; }
        let better = code > code_count(&fallback) || (code == code_count(&fallback) && services.len() > fallback.len());
        if better { fallback = services; }
    }
    if code_count(&fallback) == 0 {
        let described = language::describe_directory(root);
        if let Some(manifest) = described.manifest {
            fallback.push(Service {
                name: root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                root: Some(".".into()), language: described.language, entry_points: described.entry_points,
                role: ServiceRole::Code, discovered_by: ServiceSource::Root,
                evidence: Evidence { file: manifest.file.clone(), line: None, detail: Some(manifest.file) },
                image: None, package_name: described.package_name,
            });
        }
    }
    sort_services(&mut fallback);
    DiscoveryResult { services: fallback, strategy: None, attempted }
}
```
`sort_services`: `sort_by(|a, b| role_rank(a).cmp(&role_rank(b)).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())).then(a.name.cmp(&b.name)))` where Code ranks 0, Infrastructure 1.

- [ ] **Step 4: Implement compose**

Port `src/discover/docker-compose.ts` rule for rule:
1. `files = index.files_matching(&COMPOSE_PATTERNS, &[])`; empty -> return empty.
2. `dirs = DirectoryIndex::build(index)`; `found: IndexMap<String, Service>`.
3. Per file (sorted): `text = read_text`; `docs = parse_documents`; take `docs[0]`; `services = doc.get("services")` must be a map. `compose_dir = root.join(file).parent()`. `env = load_compose_env(compose_dir)`.
4. Per `(key, def)` entry: `name = key.as_scalar_string()`, `line = Some(key.line)`. `image = def.get("image").and_then(as_scalar_string).map(|v| interpolate(&v, &env))`.
5. `build = def.get("build")`: string -> `context = interpolate`; map -> `context` from `context`, `dockerfile` from `dockerfile` (both interpolated); if `context` is None and `dockerfile` is Some, `context = Some(".")`.
6. Root and detail:
   - `context` Some: `context_abs = compose_dir.join(context)` normalised with `path_clean` semantics (implement a small `normalize(path)` that resolves `.` and `..` lexically; do not use canonicalize because the directory may not exist). `detail = format!("build: {context}")`. If `dockerfile` Some: `dockerfile_abs = normalize(context_abs.join(dockerfile))`, `dockerfile_dir = parent`; if `dockerfile_dir != context_abs && is_file(dockerfile_abs)` -> `service_root = rel(root, dockerfile_dir)`, `detail = format!("build: {context}, dockerfile: {dockerfile}")`. If still None and `context_abs == root`: `matched = dirs.matching(&[Some(name)])`; if `Some(m) && m != "."` -> `service_root = m`, `detail = format!("build: {context}, matched directory {m}")`. If still None and `is_dir(context_abs)` -> `service_root = rel(root, context_abs)`.
   - `context` None: `service_root = dirs.matching(&[Some(name), image_basename(image)])`; `detail = image.map(|i| format!("image: {i}")).unwrap_or("service")`.
7. `described = service_root.map(|r| describe_directory(root.join(r)))`. Build the `Service` with `role` Code when root is Some else Infrastructure, `discovered_by: Strategy(DockerCompose)`, `evidence { file, line, detail: Some(detail) }`, `image`, `package_name`.
8. Insert into `found` unless an entry exists, except that a Code entry replaces an Infrastructure one.
9. Return `found.into_values().collect()`.

Note `rel(root, compose_dir.join(context))` compares against the canonical root; keep `root` canonicalised in `analyze` (Task 11) so equality checks like `context_abs == root` hold.

- [ ] **Step 5: Run tests, fmt, clippy**

Run: `cargo test -p blastradius-core --test discovery && cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: 6 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/blastradius-core
git commit -m "feat(rust): docker-compose discovery and the strategy runner"
```

---

### Task 8: Kubernetes strategy

**Files:**
- Create: `crates/blastradius-core/src/discover/kubernetes.rs`
- Modify: `discover/mod.rs` (add module and `(DiscoveryStrategy::Kubernetes, kubernetes::discover_from_kubernetes)` second in `STRATEGIES`)
- Test: append to `tests/discovery.rs`

**Interfaces:**
- Produces: `pub fn discover_from_kubernetes(root: &Path, index: &FileIndex) -> Vec<Service>;`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn kubernetes_matches_workloads_to_directories() {
    let r = discover("k8s-app");
    assert_eq!(r.strategy, Some(DiscoveryStrategy::Kubernetes));
    let s = by_name(&r.services);
    assert_eq!(s.keys().cloned().collect::<Vec<_>>(), vec!["cartservice", "frontend", "redis-cart"]);
    let f = &s["frontend"];
    assert_eq!((f.root.as_deref(), f.language.as_deref(), f.role, f.image.as_deref()), (Some("src/frontend"), Some("go"), ServiceRole::Code, Some("acme/frontend:1.0")));
    assert_eq!(f.evidence, Evidence { file: "kubernetes/frontend.yaml".into(), line: Some(9), detail: Some("Deployment, image: acme/frontend:1.0".into()) });
    assert_eq!((s["cartservice"].root.as_deref(), s["cartservice"].language.as_deref()), (Some("src/cartservice"), Some("csharp")));
    assert_eq!((s["redis-cart"].root.as_deref(), s["redis-cart"].role, s["redis-cart"].image.as_deref()), (None, ServiceRole::Infrastructure, Some("redis:alpine")));
}

#[test]
fn kubernetes_skips_helm_templates() {
    let r = discover("k8s-app");
    assert!(!r.services.iter().any(|s| s.name.contains("Values")));
}
```

- [ ] **Step 2: Run to verify failure**

- [ ] **Step 3: Implement**

Port `src/discover/kubernetes.ts`:
- `WORKLOAD_KINDS = [Deployment, StatefulSet, DaemonSet, Job, CronJob, Rollout]`, `KINDS = WORKLOAD_KINDS + Service`.
- `files = index.files_matching(&["**/*.{yml,yaml}"], &COMPOSE_PATTERNS)`.
- Per file: `text`; skip unless regex `(?m)^kind:\s*\S+` matches; skip if `text.contains("{{")`; `parse_documents`.
- Per doc that is a map: `kind = get("kind").as_str()` in KINDS; `name = get("metadata").get("name").as_scalar_string()` required. `workload = WORKLOAD_KINDS.contains(kind)`. `image = workload.then(|| first_image(doc, 0))`. `service_root = dirs.matching(&[Some(name), image_basename(image)])`. Evidence `{ file, line: Some(doc.line), detail: image.map(|i| format!("{kind}, image: {i}")).unwrap_or(kind) }`.
- `first_image(node, depth)`: if depth > 8 or not a map -> None; if `get("containers")` is a Seq, return the first item's `image` string; else recurse into each map value in order.
- Keep rule: replace when no previous, or `workload && !prev.workload`, or prev Infrastructure and new Code. Track `workload` beside each stored service.

- [ ] **Step 4: Run tests, fmt, clippy**

- [ ] **Step 5: Commit**

```bash
git add crates/blastradius-core
git commit -m "feat(rust): kubernetes discovery"
```

---

### Task 9: Monorepo strategy

**Files:**
- Create: `crates/blastradius-core/src/discover/monorepo.rs`
- Modify: `discover/mod.rs` (third entry in `STRATEGIES`)
- Test: append to `tests/discovery.rs`

**Interfaces:**
- Produces: `pub const MONOREPO_PARENTS: [&str; 5] = ["services", "apps", "packages", "microservices", "src"]; pub fn discover_from_monorepo(root: &Path, index: &FileIndex) -> Vec<Service>;`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn monorepo_takes_children_with_manifests() {
    let r = discover("monorepo-app");
    assert_eq!(r.strategy, Some(DiscoveryStrategy::Monorepo));
    let s = by_name(&r.services);
    assert_eq!(s.keys().cloned().collect::<Vec<_>>(), vec!["api", "web", "worker"]);
    assert_eq!((s["api"].root.as_deref(), s["api"].language.as_deref(), s["api"].entry_points.clone()), (Some("services/api"), Some("typescript"), vec!["src/server.ts".to_string()]));
    assert_eq!((s["worker"].root.as_deref(), s["worker"].language.as_deref(), s["worker"].entry_points.clone()), (Some("services/worker"), Some("python"), vec!["main.py".to_string()]));
    assert_eq!((s["web"].root.as_deref(), s["web"].language.as_deref()), (Some("apps/web"), Some("javascript")));
    assert_eq!(s["api"].evidence, Evidence { file: "services/api/package.json".into(), line: None, detail: Some("package.json".into()) });
}
```

- [ ] **Step 2: Run to verify failure**

- [ ] **Step 3: Implement**

For each parent in order: `parent_abs = root.join(parent)`; skip unless dir. `read_dir`, keep directories whose name does not start with `.` and is not `node_modules`, sorted by name. For each child: `describe_directory`; skip without manifest; skip if the name is already in `found`. Service: `root: Some(format!("{parent}/{child}"))`, `evidence { file: format!("{parent}/{child}/{manifest.file}"), line: None, detail: Some(manifest.file) }`, `discovered_by: Strategy(Monorepo)`, role Code.

- [ ] **Step 4: Run tests, fmt, clippy**

- [ ] **Step 5: Commit**

```bash
git add crates/blastradius-core
git commit -m "feat(rust): monorepo discovery"
```

---

### Task 10: Workspace strategy and the single-service fallback

**Files:**
- Create: `crates/blastradius-core/src/discover/workspace.rs`
- Modify: `discover/mod.rs` (fourth entry in `STRATEGIES`)
- Test: append to `tests/discovery.rs`; unit test in `workspace.rs`

**Interfaces:**
- Produces: `pub fn discover_from_workspace(root: &Path, index: &FileIndex) -> Vec<Service>; pub fn cargo_members(toml_text: &str) -> Vec<String>;`

- [ ] **Step 1: Write the failing tests**

Integration:
```rust
#[test]
fn workspace_expands_pnpm_globs_and_negations() {
    let r = discover("workspace-app");
    assert_eq!(r.strategy, Some(DiscoveryStrategy::Workspace));
    let s = by_name(&r.services);
    assert_eq!(s.keys().cloned().collect::<Vec<_>>(), vec!["billing", "gateway"]);
    assert_eq!((s["billing"].root.as_deref(), s["billing"].language.as_deref(), s["billing"].package_name.as_deref()), (Some("components/billing"), Some("typescript"), Some("@acme/billing")));
    assert_eq!(s["gateway"].evidence, Evidence { file: "pnpm-workspace.yaml".into(), line: None, detail: Some("components/*".into()) });
    assert_eq!(r.attempted.iter().map(|a| a.services).collect::<Vec<_>>(), vec![0, 0, 0, 2]);
}

#[test]
fn workspace_reads_package_json_workspaces() {
    let r = discover("npm-workspace-app");
    assert_eq!(r.strategy, Some(DiscoveryStrategy::Workspace));
    assert_eq!(r.services.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), vec!["alpha", "beta"]);
    assert_eq!(r.services[0].evidence.file, "package.json");
}

#[test]
fn single_service_fallback_reports_attempts() {
    let r = discover("single-app");
    assert_eq!(r.strategy, None);
    assert_eq!(r.attempted.iter().map(|a| (a.strategy, a.services)).collect::<Vec<_>>(), vec![
        (DiscoveryStrategy::DockerCompose, 0), (DiscoveryStrategy::Kubernetes, 0), (DiscoveryStrategy::Monorepo, 0), (DiscoveryStrategy::Workspace, 0)]);
    assert_eq!(r.services.len(), 1);
    let s = &r.services[0];
    assert_eq!((s.name.as_str(), s.root.as_deref(), s.language.as_deref(), s.role, s.discovered_by), ("single-app", Some("."), Some("typescript"), ServiceRole::Code, ServiceSource::Root));
}
```
Unit, in `workspace.rs`:
```rust
#[test]
fn parses_cargo_workspace_members() {
    let toml = "[package]\nname = \"root\"\n\n[workspace]\nmembers = [\n  \"crates/api\", # the api\n  'crates/worker',\n  \"tools/*\"\n]\n";
    assert_eq!(cargo_members(toml), vec!["crates/api", "crates/worker", "tools/*"]);
    assert_eq!(cargo_members("[package]\nname = 'x'"), Vec::<String>::new());
    assert_eq!(cargo_members("not = [valid"), Vec::<String>::new());
}
```

- [ ] **Step 2: Run to verify failure**

- [ ] **Step 3: Implement**

- Collect `patterns: Vec<(pattern, file)>`: `package.json` `workspaces` (array, or object with `packages` array) -> file "package.json"; `pnpm-workspace.yaml` `packages` -> "pnpm-workspace.yaml"; `cargo_members(Cargo.toml)` -> "Cargo.toml".
- `cargo_members`: `toml::from_str::<toml::Table>`; `["workspace"]["members"]` array of strings; anything else -> empty.
- `dirs: IndexMap<String, (pattern, file)>`: first every `**/project.json` from the index whose parent is not "." maps `parent -> (pj, pj)`. Then `exclude = patterns starting with '!'` (stripped); for each non-negated pattern, `index.dirs_matching(&[pattern], &exclude)`; insert dirs not "." and not already present.
- Sort by dir name (byte order). For each: `describe_directory`; skip without manifest; `name = basename`; skip clashes. Service with `root: Some(dir)`, `evidence { file, line: None, detail: Some(pattern) }`, `discovered_by: Strategy(Workspace)`.

- [ ] **Step 4: Run all tests, fmt, clippy**

Run: `cargo test -p blastradius-core && cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: all discovery tests pass, including the fallback test now that all four strategies are wired.

- [ ] **Step 5: Commit**

```bash
git add crates/blastradius-core
git commit -m "feat(rust): workspace discovery and the single-service fallback"
```

---

### Task 11: Graph, analyze, JSON

**Files:**
- Create: `crates/blastradius-core/src/graph.rs`, `crates/blastradius-core/src/analyze.rs`, `crates/blastradius-core/tests/analyze.rs`
- Modify: `lib.rs`

**Interfaces:**
- Produces:
  ```rust
  // graph.rs
  #[derive(Debug, Default, Clone)]
  pub struct BlastGraph { services: IndexMap<String, Service>, edges: Vec<Edge> }
  impl BlastGraph {
      pub fn new() -> Self;
      pub fn add_service(&mut self, s: Service);                 // replaces by name
      pub fn has_service(&self, name: &str) -> bool;
      pub fn service(&self, name: &str) -> Option<&Service>;
      pub fn add_edge(&mut self, e: Edge) -> Result<(), GraphError>;  // GraphError::UnknownService { name, source, target }
      pub fn services(&self) -> Vec<Service>;                    // sorted via discover::sort_services
      pub fn edges(&self) -> &[Edge];
      pub fn inbound(&self, name: &str) -> Vec<&Edge>;           // edges whose target is name
      pub fn outbound(&self, name: &str) -> Vec<&Edge>;
      pub fn size(&self) -> (usize, usize);                      // (services, edges)
  }
  // analyze.rs
  pub struct Analysis { pub repository: String, pub root: PathBuf, pub discovery: Discovery, pub graph: BlastGraph, pub runtime: Runtime }
  #[derive(Serialize)] pub struct Discovery { pub strategy: Option<DiscoveryStrategy>, pub attempted: Vec<DiscoveryAttempt> }
  #[derive(Serialize)] pub struct Runtime { pub connected: bool }   // always false in this plan
  #[derive(Serialize)] pub struct AnalysisJson { repository, root: String, discovery, services: Vec<Service>, edges: Vec<Edge>, runtime }
  #[derive(Debug, thiserror::Error)] pub enum AnalyzeError { #[error("not a directory: {0}")] NotADirectory(String), #[error(transparent)] Io(#[from] std::io::Error) }
  pub fn analyze(root: &Path) -> Result<Analysis, AnalyzeError>;
  pub fn repository_name(root: &Path) -> String;   // owner/repo from origin, else directory basename
  impl Analysis { pub fn to_json(&self) -> AnalysisJson; }
  ```

- [ ] **Step 1: Write the failing tests**

`tests/analyze.rs`:
```rust
mod common;
use blastradius::*;
use common::fixture;
use pretty_assertions::assert_eq;

fn service(name: &str) -> Service {
    Service { name: name.into(), root: Some(name.into()), language: Some("go".into()), entry_points: vec![], role: ServiceRole::Code,
        discovered_by: ServiceSource::Strategy(DiscoveryStrategy::Monorepo), evidence: Evidence { file: format!("{name}/go.mod"), line: None, detail: None }, image: None, package_name: None }
}
fn edge(s: &str, t: &str, ty: EdgeType, c: Confidence) -> Edge { Edge { source: s.into(), target: t.into(), edge_type: ty, confidence: c, evidence: vec![] } }

#[test]
fn graph_stores_services_and_answers_inbound() {
    let mut g = BlastGraph::new();
    for n in ["checkout", "orders", "payments"] { g.add_service(service(n)); }
    g.add_edge(edge("checkout", "orders", EdgeType::Http, Confidence::Static)).unwrap();
    g.add_edge(edge("payments", "orders", EdgeType::Event, Confidence::Inferred)).unwrap();
    assert_eq!(g.size(), (3, 2));
    let mut inbound: Vec<&str> = g.inbound("orders").iter().map(|e| e.source.as_str()).collect(); inbound.sort();
    assert_eq!(inbound, vec!["checkout", "payments"]);
    assert!(g.outbound("orders").is_empty());
    assert_eq!(g.services().iter().map(|s| s.name.clone()).collect::<Vec<_>>(), vec!["checkout", "orders", "payments"]);
}

#[test]
fn graph_keeps_parallel_edges_and_refuses_unknown_services() {
    let mut g = BlastGraph::new();
    g.add_service(service("a")); g.add_service(service("b"));
    g.add_edge(edge("a", "b", EdgeType::Http, Confidence::Static)).unwrap();
    g.add_edge(edge("a", "b", EdgeType::Import, Confidence::Static)).unwrap();
    assert_eq!(g.edges().len(), 2);
    let err = g.add_edge(edge("a", "ghost", EdgeType::Http, Confidence::Static)).unwrap_err();
    assert!(err.to_string().contains("Unknown service \"ghost\""));
}

#[test]
fn analyze_builds_a_graph_of_discovered_services() {
    let a = analyze(&fixture("compose-app")).unwrap();
    assert!(!a.repository.is_empty());
    assert_eq!(a.discovery.strategy, Some(DiscoveryStrategy::DockerCompose));
    assert_eq!(a.graph.size(), (3, 0));
    assert!(!a.runtime.connected);
}

#[test]
fn analyze_serialises_to_the_contract() {
    let json = serde_json::to_value(analyze(&fixture("monorepo-app")).unwrap().to_json()).unwrap();
    assert_eq!(json["services"].as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap()).collect::<Vec<_>>(), vec!["api", "web", "worker"]);
    assert_eq!(json["edges"], serde_json::json!([]));
    assert_eq!(json["runtime"], serde_json::json!({ "connected": false }));
    assert_eq!(json["discovery"]["strategy"], "monorepo");
    assert_eq!(json["discovery"]["attempted"][0], serde_json::json!({ "strategy": "docker-compose", "services": 0 }));
    let keys: Vec<&str> = json.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["repository", "root", "discovery", "services", "edges", "runtime"]);
}

#[test]
fn analyze_rejects_a_file_path() {
    let err = analyze(&fixture("compose-app").join("docker-compose.yml")).unwrap_err();
    assert!(err.to_string().contains("not a directory"));
}
```

- [ ] **Step 2: Run to verify failure**

- [ ] **Step 3: Implement**

- `analyze`: `if !is_dir(root) -> NotADirectory(root.display())`; `root = root.canonicalize()?`; `index = FileIndex::build(&root)`; `discovery = discover_services(&root, &index)`; graph from services; `repository = repository_name(&root)`.
- `repository_name`: `Command::new("git").args(["-C", root, "remote", "get-url", "origin"]).output()`; on success apply regex `[:/]([^/:\s]+)/([^/\s]+?)(?:\.git)?/?$` to the trimmed stdout -> `owner/repo`; else basename of root.
- `to_json`: `root` as `display().to_string()`. Serialize `AnalysisJson` with `#[serde(rename_all = "camelCase")]`; fields declared in the contract order.
- `serde_json::to_string_pretty` output is what the CLI prints (two-space indent matches the TypeScript `JSON.stringify(_, null, 2)`).

- [ ] **Step 4: Run tests, fmt, clippy**

- [ ] **Step 5: Commit**

```bash
git add crates/blastradius-core
git commit -m "feat(rust): graph, analyze and the JSON contract"
```

---

### Task 12: Terminal report

**Files:**
- Create: `crates/blastradius-core/src/report.rs`
- Modify: `lib.rs`
- Test: append to `tests/analyze.rs`

**Interfaces:**
- Produces: `pub fn format_repo_report(analysis: &Analysis, color: bool) -> String;`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn report_shows_findings_and_says_what_is_not_built() {
    let r = format_repo_report(&analyze(&fixture("compose-app")).unwrap(), false);
    assert!(r.contains("BLAST RADIUS"));
    assert!(r.contains("2 detected  (docker-compose)"));
    assert!(r.contains("not connected - static only"));
    let re = regex::Regex::new(r"checkout\s+typescript\s+services/checkout\s+docker-compose\.yml:2").unwrap();
    assert!(re.is_match(&r), "{r}");
    assert!(r.contains("1 declared but not built here (images): redis"));
    assert!(r.contains("Dependency mapping is not built yet"));
    assert!(!r.contains('\u{1b}'));
}

#[test]
fn report_is_honest_about_single_and_deploy_only() {
    let r = format_repo_report(&analyze(&fixture("single-app")).unwrap(), false);
    assert!(r.contains("This looks like a single service."));
    assert!(r.contains("docker-compose 0") && r.contains("workspace 0"));
    let r = format_repo_report(&analyze(&fixture("deploy-only-app")).unwrap(), false);
    assert!(r.contains("declares 3 services but builds none of them here"));
    assert!(r.contains("3 declared but not built here (images): carts, carts-db, front-end"));
}

#[test]
fn report_colours_only_when_asked() {
    assert!(format_repo_report(&analyze(&fixture("compose-app")).unwrap(), true).contains('\u{1b}'));
}
```
Add `regex.workspace = true` under `[dev-dependencies]` of the core crate.

- [ ] **Step 2: Run to verify failure**

- [ ] **Step 3: Implement**

Port `src/report/terminal.ts` line for line. Colour helpers: `fn bold(s, color)`, `fn dim(s, color)`, `fn yellow(s, color)` returning `String`, using `owo_colors::OwoColorize` (`s.bold().to_string()`) when `color` else `s.to_string()`. Sections and exact strings:
- Header rows `row(label, value)` = `format!("  {label:<13} {value}")`.
- `Services`: `"{n} detected  (strategy)"` with the parenthesis dimmed when a strategy exists, else `"{n} detected"`.
- `Runtime`: dim `"not connected - static only"`.
- No strategy: yellow lines, either the deploy-only message (`This repository declares {n} services but builds none of them here,` / `so there is no code to trace. Run blast-radius on the repository that` / `holds the services.`) when there is no code and some infrastructure, else the single-service message (`This looks like a single service. Blast radius analysis needs a` / `multi-service repository.`); then `row("Tried", dim(attempts joined by " · " as "{strategy} {services}"))`.
- SERVICES table: columns NAME, LANGUAGE, ROOT, EVIDENCE; widths `max(4, names)`, `max(8, languages)`, `max(4, roots)`; root cell is `root`, or `(image {image})`, or `(no directory)`; evidence `file:line` or `file`; infrastructure rows dimmed whole; header dimmed. After the table: dim `"{n} declared but not built here (images): {names joined by ", "}"` when any infrastructure.
- STRUCTURE: the three dimmed lines from the TypeScript source verbatim.
- Join with `\n`, no trailing newline (the CLI adds it).

- [ ] **Step 4: Run tests, fmt, clippy**

- [ ] **Step 5: Commit**

```bash
git add crates/blastradius-core
git commit -m "feat(rust): terminal repository report"
```

---

### Task 13: CLI

**Files:**
- Modify: `crates/blastradius-cli/src/main.rs`
- Create: `crates/blastradius-cli/tests/cli.rs`

**Interfaces:**
- Consumes: `blastradius::{analyze, format_repo_report}`.
- Produces: the `blast-radius` binary. `blast-radius analyze [PATH] [--json] [--no-color]`, `blast-radius --version`. Errors print `blast-radius: <message>` to stderr and exit 1.

- [ ] **Step 1: Write the failing tests**

```rust
use std::path::PathBuf;
use std::process::Command;

fn bin() -> Command { Command::new(env!("CARGO_BIN_EXE_blast-radius")) }
fn fixture(name: &str) -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures").join(name) }

#[test]
fn analyze_json_prints_the_contract() {
    let out = bin().args(["analyze", fixture("compose-app").to_str().unwrap(), "--json"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["discovery"]["strategy"], "docker-compose");
    assert_eq!(json["services"].as_array().unwrap().len(), 3);
}

#[test]
fn analyze_report_defaults_to_the_current_directory_and_has_no_colour_when_piped() {
    let out = bin().arg("analyze").current_dir(fixture("monorepo-app")).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success());
    assert!(text.contains("3 detected  (monorepo)"));
    assert!(!text.contains('\u{1b}'));
}

#[test]
fn errors_go_to_stderr_with_exit_code_1() {
    let out = bin().args(["analyze", fixture("compose-app").join("docker-compose.yml").to_str().unwrap()]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).starts_with("blast-radius: not a directory"));
}

#[test]
fn version_flag() {
    let out = bin().arg("--version").output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("blast-radius 0.0.1"));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p blastradius-cli`

- [ ] **Step 3: Implement**

```rust
use clap::{Parser, Subcommand};
use std::io::{IsTerminal, Write};

#[derive(Parser)]
#[command(name = "blast-radius", version, about = "Which services can this change reach?")]
struct Cli { #[command(subcommand)] command: Cmd }

#[derive(Subcommand)]
enum Cmd {
    /// Analyse a repository: its services, and what a change can reach
    Analyze {
        /// Repository root
        #[arg(default_value = ".")] path: std::path::PathBuf,
        /// Print the graph as JSON instead of the report
        #[arg(long)] json: bool,
        /// Disable colours
        #[arg(long = "no-color")] no_color: bool,
    },
}

fn main() {
    if let Err(err) = run() { eprintln!("blast-radius: {err}"); std::process::exit(1); }
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Cmd::Analyze { path, json, no_color } => {
            let analysis = blastradius::analyze(&path)?;
            let mut out = std::io::stdout().lock();
            if json { writeln!(out, "{}", serde_json::to_string_pretty(&analysis.to_json())?)?; return Ok(()); }
            let color = !no_color && std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
            writeln!(out, "{}", blastradius::format_repo_report(&analysis, color))?;
            Ok(())
        }
    }
}
```

- [ ] **Step 4: Run tests, fmt, clippy**

Run: `cargo test && cargo fmt --check && cargo clippy --all-targets -- -D warnings`

- [ ] **Step 5: Commit**

```bash
git add crates/blastradius-cli
git commit -m "feat(rust): blast-radius CLI"
```

---

### Task 14: Parity against the TypeScript baseline, fixtures and corpus

**Files:**
- Create: `test/expected/discovery/<fixture>.json` (10 files), `test/expected/corpus/<repo>.json` (8 files), `crates/blastradius-core/tests/parity.rs`, `crates/blastradius-core/tests/corpus.rs`

**Interfaces:**
- `test/expected/corpus/<repo>.json` shape (hand-derived from the baseline JSON):
  ```json
  { "strategy": "docker-compose", "code": ["cart", "catalogue", ...], "infrastructure": ["rabbitmq", "redis"] }
  ```

- [ ] **Step 1: Copy the baseline**

```bash
S=/private/tmp/claude-501/-Users-soumyaranjanpanda-Dream-YC-Vernier-OSS/fd8a3efa-b35e-4c97-9072-c61d885be215/scratchpad/baseline
mkdir -p test/expected/discovery test/expected/corpus
cp $S/fixtures/*.json test/expected/discovery/
for f in $S/corpus/*.json; do n=$(basename $f .json); node -e '
const j=require(process.argv[1]);const names=r=>j.services.filter(s=>s.role===r).map(s=>s.name).sort();
console.log(JSON.stringify({strategy:j.discovery.strategy,code:names("code"),infrastructure:names("infrastructure")},null,2))' $f > test/expected/corpus/$n.json; done
```

- [ ] **Step 2: Write the parity test**

`tests/parity.rs`:
```rust
mod common;
use common::fixture;
use pretty_assertions::assert_eq;
use serde_json::Value;
use std::fs;

fn normalise(mut v: Value) -> Value {
    let obj = v.as_object_mut().unwrap();
    obj.remove("root"); obj.remove("repository");
    if let Some(services) = obj.get_mut("services").and_then(Value::as_array_mut) {
        services.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    }
    v
}

#[test]
fn every_fixture_matches_the_typescript_baseline() {
    let dir = fixture("..").join("expected/discovery");
    let mut checked = 0;
    for entry in fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") { continue; }
        let name = path.file_stem().unwrap().to_str().unwrap();
        let expected: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let actual = serde_json::to_value(blastradius::analyze(&fixture(name)).unwrap().to_json()).unwrap();
        assert_eq!(normalise(actual), normalise(expected), "fixture {name}");
        checked += 1;
    }
    assert_eq!(checked, 10);
}
```

- [ ] **Step 3: Write the corpus test**

`tests/corpus.rs`:
```rust
mod common;
use pretty_assertions::assert_eq;
use serde::Deserialize;
use std::{fs, path::Path, time::Instant};

#[derive(Deserialize)]
struct Expected { strategy: Option<String>, code: Vec<String>, infrastructure: Vec<String> }

#[test]
fn corpus_service_discovery_matches_expected() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let corpus = repo_root.join("corpus");
    if !corpus.is_dir() { eprintln!("corpus/ missing: run `pnpm corpus` or scripts/corpus.sh; skipping"); return; }
    let mut failures = Vec::new();
    for entry in fs::read_dir(repo_root.join("test/expected/corpus")).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_stem().unwrap().to_str().unwrap().to_string();
        let repo = corpus.join(&name);
        if !repo.is_dir() { eprintln!("corpus/{name} missing; skipping"); continue; }
        let expected: Expected = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let started = Instant::now();
        let a = blastradius::analyze(&repo).unwrap();
        let elapsed = started.elapsed();
        let json = a.to_json();
        let mut code: Vec<String> = json.services.iter().filter(|s| s.role == blastradius::ServiceRole::Code).map(|s| s.name.clone()).collect(); code.sort();
        let mut infra: Vec<String> = json.services.iter().filter(|s| s.role == blastradius::ServiceRole::Infrastructure).map(|s| s.name.clone()).collect(); infra.sort();
        let strategy = json.discovery.strategy.map(|s| serde_json::to_value(s).unwrap().as_str().unwrap().to_string());
        eprintln!("{name:<32} {:>4} code {:>4} infra  {:>6.1} ms", code.len(), infra.len(), elapsed.as_secs_f64() * 1000.0);
        if (strategy.clone(), code.clone(), infra.clone()) != (expected.strategy.clone(), expected.code.clone(), expected.infrastructure.clone()) {
            failures.push(format!("{name}: strategy {strategy:?} vs {:?}\n  code   {:?}\n  expect {:?}\n  infra  {:?}\n  expect {:?}", expected.strategy, code, expected.code, infra, expected.infrastructure));
        }
        assert!(elapsed.as_millis() < 500, "{name} took {elapsed:?}");
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
```

- [ ] **Step 4: Run both and fix every difference**

Run: `cargo test -p blastradius-core --test parity --test corpus -- --nocapture`

Expected on first run: probably a handful of differences. Known likely causes and the fix for each:
- Ordering only: the parity test already sorts by name; if `entryPoints` differ in order, sort them in `detect_entry_points`.
- Line off by one in YAML: adjust `Marker::line()` handling in `yaml.rs`.
- `.gitignore` honoured by the walk but not by the TypeScript engine: a corpus repo may lose or gain a service. Check the repo's `.gitignore`. If the ignored path holds real service code, set `.git_ignore(false)` on the walker and record the decision in the spec's "Stage 1 port" section. If it holds build output, update the expected file and note the improvement in the commit message.
- Directories `bin`/`build` inside the fixed ignore list appearing as services in the baseline: the TypeScript glob ignore did not exclude the directory entry itself. Match the baseline: only skip descending into ignored directories for files, and keep them listed in `dirs`, except `node_modules` and `.git`.
- Fix the engine, never the expected file, unless the difference is a documented improvement.

- [ ] **Step 5: Run everything, fmt, clippy**

Run: `cargo test && cargo fmt --check && cargo clippy --all-targets -- -D warnings`

- [ ] **Step 6: Commit**

```bash
git add test/expected crates/blastradius-core/tests
git commit -m "test(rust): parity with the TypeScript baseline on fixtures and corpus"
```

---

### Task 15: CI, README, remove the TypeScript engine

**Files:**
- Modify: `.github/workflows/ci.yml`, `README.md`, `package.json`, `scripts/corpus.sh`
- Delete: `src/`, `test/analyze.test.ts`, `test/discover.test.ts`, `test/graph.test.ts`, `tsup.config.ts`, `vitest.config.ts`, `tsconfig.json`, `pnpm-lock.yaml`, `dist/`

- [ ] **Step 1: Rewrite CI**

`.github/workflows/ci.yml`:
```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

jobs:
  test:
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all --check
      - run: cargo clippy --all-targets -- -D warnings
      - run: cargo test
      - name: Smoke test the CLI on this repository
        run: cargo run -q -- analyze .
```

- [ ] **Step 2: Update the README**

Replace the "Run it from source" block with:
```bash
git clone https://github.com/Go-Vernier/Vernier-OSS.git
cd Vernier-OSS
cargo build --release
./target/release/blast-radius analyze /path/to/a/repository
./target/release/blast-radius analyze /path/to/a/repository --json
```
Replace the "Developing" section with:
```bash
cargo test                      # fixtures under test/fixtures, parity against the baseline
cargo fmt --check && cargo clippy --all-targets -- -D warnings
sh scripts/corpus.sh            # shallow-clone the eight reference repositories into corpus/
cargo test --test corpus -- --nocapture   # discovery counts and timing on the corpus
```
Replace the library example with:
```rust
use blastradius::{analyze, format_repo_report};

let analysis = analyze(std::path::Path::new("./my-repo"))?;
println!("{}", format_repo_report(&analysis, false));
```
Add one sentence under Status: "The engine is Rust; the npm package `blastradius` will wrap the binary when it is published."

- [ ] **Step 3: Trim package.json and fix the corpus script**

`package.json`:
```json
{
  "name": "blastradius",
  "version": "0.0.1",
  "description": "Which services can this change reach? Static analysis of the repository joined with production traces, at pull request time.",
  "license": "MIT",
  "private": true,
  "repository": { "type": "git", "url": "git+https://github.com/Go-Vernier/Vernier-OSS.git" },
  "homepage": "https://github.com/Go-Vernier/Vernier-OSS#readme",
  "bugs": "https://github.com/Go-Vernier/Vernier-OSS/issues",
  "keywords": ["blast-radius", "microservices", "dependency-graph", "static-analysis", "opentelemetry", "pull-request", "impact-analysis"],
  "scripts": { "corpus": "sh scripts/corpus.sh" }
}
```
`private: true` until the wrapper exists, so a stray publish cannot ship an empty package. In `scripts/corpus.sh` change the last line to `echo "Done. Run: cargo run -- analyze corpus/<name>"`.

- [ ] **Step 4: Delete the TypeScript engine**

```bash
git rm -r src test/analyze.test.ts test/discover.test.ts test/graph.test.ts tsup.config.ts vitest.config.ts tsconfig.json pnpm-lock.yaml
rm -rf dist node_modules
```

- [ ] **Step 5: Verify**

Run: `cargo test && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo run -q -- analyze . && cargo run -q -- analyze corpus/robot-shop`
Expected: all green; the repository report for this repo says single service (Cargo workspace members are `crates/*`, two members, both with manifests: expect `2 detected  (workspace)`; either outcome is fine as long as it is true).

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "chore: Rust engine replaces the TypeScript engine; CI on cargo"
```

---

## Self-review

- Spec coverage: Repository shape (Tasks 1, 15), JSON contract (Tasks 1, 11, 14), Stage 1 port behaviours (Tasks 4 to 10; anchors and merge keys Task 3; TOML members Task 10; `.gitignore` decision Task 14), validation for discovery parity and corpus service lists (Task 14), performance measurement (Task 14 asserts under 500 ms). The `mapping` block, edges and Stage 2 sections belong to Plan B.
- Placeholders: none. Every test is written out; implementation steps name the rules and the TypeScript source they port, which stays in the tree until Task 15.
- Types: `Service`, `Evidence`, `ServiceRole`, `ServiceSource`, `DiscoveryStrategy` defined in Task 1 and used identically after. `FileIndex` methods named in Task 2 are the ones called in Tasks 6 to 10. `discover_services(root, index)` in Task 7 is what `analyze` calls in Task 11 and the test helper in Task 7 uses. `to_json()` in Task 11 is what the CLI (Task 13) and the parity test (Task 14) call.
