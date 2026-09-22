//! One walk over the repository, reused by every stage.
//!
//! Hidden entries, the fixed ignore list and the repository's own
//! `.gitignore` are skipped. Nothing outside the analysed directory is read,
//! so two people analysing the same commit get the same answer.
use std::fs;
use std::path::{Component, Path, PathBuf};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;

/// Directories that never hold a service declaration worth reading.
pub const IGNORE_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "vendor",
    "dist",
    "build",
    "target",
    ".next",
    "__pycache__",
    ".venv",
    "venv",
    "bin",
    "obj",
];

/// Every file and directory under the root, as repository-relative POSIX
/// paths, sorted.
#[derive(Debug, Clone)]
pub struct FileIndex {
    root: PathBuf,
    files: Vec<String>,
    dirs: Vec<String>,
}

impl FileIndex {
    pub fn build(root: &Path) -> Self {
        let mut files = Vec::new();
        let mut dirs = Vec::new();
        let walker = WalkBuilder::new(root)
            .hidden(true)
            .parents(false)
            .git_ignore(true)
            .git_global(false)
            .git_exclude(false)
            .follow_links(false)
            .filter_entry(|entry| {
                entry.depth() == 0
                    || !IGNORE_DIRS.contains(&entry.file_name().to_string_lossy().as_ref())
            })
            .build();
        for entry in walker.flatten() {
            if entry.depth() == 0 {
                continue;
            }
            let Some(relative) = rel(root, entry.path()) else {
                continue;
            };
            match entry.file_type() {
                Some(kind) if kind.is_dir() => dirs.push(relative),
                Some(kind) if kind.is_file() => files.push(relative),
                _ => {}
            }
        }
        files.sort();
        dirs.sort();
        Self {
            root: root.to_path_buf(),
            files,
            dirs,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn files(&self) -> &[String] {
        &self.files
    }

    /// Every directory except the root itself.
    pub fn dirs(&self) -> &[String] {
        &self.dirs
    }

    /// Files whose relative path matches any `include` glob and no
    /// `exclude` glob. `*` never crosses a `/`; `**/` matches zero or more
    /// directories, so `**/x.yml` also matches `x.yml` at the root.
    pub fn files_matching(&self, include: &[&str], exclude: &[&str]) -> Vec<String> {
        matching(&self.files, include, exclude)
    }

    pub fn dirs_matching(&self, include: &[&str], exclude: &[&str]) -> Vec<String> {
        matching(&self.dirs, include, exclude)
    }

    /// Directories at most `depth` levels down; depth 1 is the root's children.
    pub fn dirs_up_to_depth(&self, depth: usize) -> Vec<String> {
        self.dirs
            .iter()
            .filter(|d| d.split('/').count() <= depth)
            .cloned()
            .collect()
    }
}

fn glob_set(patterns: &[&str]) -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        if let Ok(glob) = GlobBuilder::new(pattern).literal_separator(true).build() {
            builder.add(glob);
        }
    }
    builder.build().unwrap_or_else(|_| GlobSet::empty())
}

fn matching(items: &[String], include: &[&str], exclude: &[&str]) -> Vec<String> {
    let include = glob_set(include);
    let exclude = glob_set(exclude);
    items
        .iter()
        .filter(|p| include.is_match(p) && !exclude.is_match(p))
        .cloned()
        .collect()
}

pub fn is_dir(p: &Path) -> bool {
    fs::metadata(p).is_ok_and(|m| m.is_dir())
}

pub fn is_file(p: &Path) -> bool {
    fs::metadata(p).is_ok_and(|m| m.is_file())
}

/// File contents, or None on any error. Invalid UTF-8 is decoded lossily.
pub fn read_text(p: &Path) -> Option<String> {
    fs::read(p)
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

pub fn read_json(p: &Path) -> Option<serde_json::Value> {
    serde_json::from_str(&read_text(p)?).ok()
}

/// Repository-relative POSIX path, "." for the root itself, or None when
/// `abs` lies outside the repository.
pub fn rel(root: &Path, abs: &Path) -> Option<String> {
    let relative = abs.strip_prefix(root).ok()?;
    if relative.as_os_str().is_empty() {
        return Some(".".to_string());
    }
    Some(posix(relative))
}

pub fn posix(p: &Path) -> String {
    p.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Resolves `.` and `..` lexically, without touching the file system, so a
/// path that does not exist yet can still be compared.
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures")
            .join(name)
            .canonicalize()
            .unwrap()
    }

    #[test]
    fn indexes_files_and_dirs_relative_posix_sorted() {
        let ix = FileIndex::build(&fixture("compose-app"));
        assert!(ix.files().contains(&"docker-compose.yml".to_string()));
        assert!(
            ix.files()
                .contains(&"services/checkout/src/index.ts".to_string())
        );
        assert_eq!(ix.dirs_up_to_depth(1), vec!["services"]);
        assert!(ix.dirs().contains(&"services/checkout/src".to_string()));
        let mut sorted = ix.files().to_vec();
        sorted.sort();
        assert_eq!(&sorted, ix.files());
    }

    #[test]
    fn glob_filters_match_whole_relative_paths() {
        let ix = FileIndex::build(&fixture("compose-app"));
        assert_eq!(
            ix.files_matching(
                &["**/docker-compose*.{yml,yaml}", "**/compose.{yml,yaml}"],
                &[]
            ),
            vec!["docker-compose.yml"]
        );
        assert_eq!(
            ix.dirs_matching(&["services/*"], &["services/legacy"]),
            vec!["services/checkout", "services/orders"]
        );
        assert_eq!(
            ix.files_matching(&["**/*.{yml,yaml}"], &["**/docker-compose*.{yml,yaml}"]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn rel_and_reads() {
        let root = fixture("compose-app");
        assert_eq!(rel(&root, &root), Some(".".into()));
        assert_eq!(
            rel(&root, &root.join("services/orders")),
            Some("services/orders".into())
        );
        assert_eq!(rel(&root, root.parent().unwrap()), None);
        assert!(is_dir(&root) && !is_file(&root));
        assert!(
            read_text(&root.join("docker-compose.yml"))
                .unwrap()
                .contains("services:")
        );
        assert_eq!(read_text(&root.join("missing")), None);
        assert_eq!(
            read_json(&root.join("services/checkout/package.json")).unwrap()["name"],
            "@acme/checkout"
        );
    }

    #[test]
    fn normalize_resolves_dots_lexically() {
        assert_eq!(
            normalize(Path::new("/a/b/./c/../d")),
            PathBuf::from("/a/b/d")
        );
        assert_eq!(normalize(Path::new("/a/b/")), PathBuf::from("/a/b"));
    }
}
