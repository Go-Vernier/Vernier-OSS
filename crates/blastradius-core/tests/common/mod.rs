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

// ------------------------------------------------------- temporary git repos

use std::process::Command;

/// A throwaway repository under the system temp directory, removed on drop.
pub struct TempRepo {
    pub root: PathBuf,
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "user.name=vernier",
            "-c",
            "user.email=vernier@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

pub fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

pub fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// Adds to an existing file, so the edges its content produces survive.
pub fn append(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str(content);
    std::fs::write(path, text).unwrap();
}

fn fresh(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("vernier-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

/// `edges-http-app` as a repository whose history holds, newest first: a
/// plain commit on payment, a docs-only commit, squash-merged PR #8 on web,
/// merged PR #7 on catalogue, the initial commit. Branch `pr-9` (cart) is
/// unmerged and exposed as `refs/pull/9/head`.
pub fn http_repo(tag: &str) -> TempRepo {
    let root = fresh(tag);
    copy_dir(&fixture("edges-http-app"), &root);
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "initial"]);

    git(&root, &["checkout", "-q", "-b", "catalogue-handler"]);
    write(&root, "catalogue/handlers.go", "package main\n\n// pr 7\n");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "catalogue: add handler"]);
    git(&root, &["checkout", "-q", "main"]);
    git(
        &root,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "Merge pull request #7 from acme/catalogue-handler",
            "catalogue-handler",
        ],
    );

    append(
        &root,
        "web/default.conf.template",
        "# pr 8
",
    );
    write(&root, "README.md", "# shop\n");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "web: tweak nginx (#8)"]);

    write(&root, "docs/notes.md", "notes\n");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "docs: notes"]);

    append(&root, "payment/payment.py", "# retry\n");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "payment: retry"]);

    git(&root, &["checkout", "-q", "-b", "pr-9"]);
    append(&root, "cart/server.js", "// pr 9\n");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "cart: coupon"]);
    git(&root, &["checkout", "-q", "main"]);
    git(&root, &["update-ref", "refs/pull/9/head", "pr-9"]);
    TempRepo { root }
}

/// `edges-events-app` copied under `nested/app/` in a repository whose two
/// commits carry no pull request markers; the second touches
/// `checkout/producer.js` and a file outside the app.
pub fn nested_repo(tag: &str) -> TempRepo {
    let root = fresh(tag);
    copy_dir(&fixture("edges-events-app"), &root.join("nested/app"));
    write(&root, "README.md", "# mono\n");
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "initial"]);
    append(&root, "nested/app/checkout/producer.js", "// v2\n");
    write(&root, "README.md", "# mono v2\n");
    git(&root, &["add", "-A"]);
    git(
        &root,
        &["commit", "-q", "-m", "checkout: publish v2 events"],
    );
    TempRepo { root }
}
