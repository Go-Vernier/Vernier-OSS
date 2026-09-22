//! The git subprocess: which files a pull request, a commit or a diff range
//! changed. Everything here reads the local repository; nothing talks to a
//! forge. A pull request must already be in the history or fetched into a
//! ref, and the error says how when it is not.
use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;

use regex::Regex;
use thiserror::Error;

use crate::blast::{Change, ChangeKind};

/// First-parent commits searched for a pull request number.
pub const LOG_LIMIT: usize = 10_000;

#[derive(Debug, Error)]
pub enum GitError {
    #[error("{0} is not inside a git repository")]
    NotARepository(String),
    #[error("git {command}: {message}")]
    Command { command: String, message: String },
    #[error(
        "pull request #{number} is not in the local history of {root}: no first-parent commit on HEAD carries it ({searched} searched) and no ref holds it. Fetch it with `git fetch origin pull/{number}/head:refs/pull/{number}/head` and run again, or pass --diff <range> or --files <paths>"
    )]
    PullRequestNotFound {
        number: u64,
        root: String,
        searched: usize,
    },
}

/// One first-parent commit on HEAD.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub hash: String,
    pub short: String,
    pub parents: usize,
    /// Committer date, `YYYY-MM-DD`.
    pub date: String,
    pub subject: String,
    /// The number the subject carries, when it was merged from a pull request.
    pub pull_request: Option<u64>,
}

fn run(root: &Path, args: &[&str]) -> Result<String, GitError> {
    let command = args.join(" ");
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|e| GitError::Command {
            command: command.clone(),
            message: e.to_string(),
        })?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if message.contains("not a git repository") {
            return Err(GitError::NotARepository(root.display().to_string()));
        }
        return Err(GitError::Command { command, message });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn is_repository(root: &Path) -> bool {
    run(root, &["rev-parse", "--is-inside-work-tree"]).is_ok_and(|s| s.trim() == "true")
}

/// The analysed root's path inside the repository: empty at the top level,
/// else `sub/dir/`.
pub fn prefix(root: &Path) -> Result<String, GitError> {
    run(root, &["rev-parse", "--show-prefix"]).map(|s| s.trim().to_string())
}

/// Paths git printed, relative to the repository top, made relative to the
/// analysed root. Returns the paths kept and how many lay outside.
pub fn relative(prefix: &str, paths: Vec<String>) -> (Vec<String>, usize) {
    if prefix.is_empty() {
        return (paths, 0);
    }
    let mut kept = Vec::new();
    let mut outside = 0;
    for p in paths {
        match p.strip_prefix(prefix) {
            Some(rest) if !rest.is_empty() => kept.push(rest.to_string()),
            _ => outside += 1,
        }
    }
    (kept, outside)
}

static MARKERS: LazyLock<[Regex; 4]> = LazyLock::new(|| {
    [
        Regex::new(r"^Merge pull request #(\d+)").unwrap(),
        Regex::new(r"\(#(\d+)\)\s*$").unwrap(),
        Regex::new(r"\(pull request #(\d+)\)").unwrap(),
        Regex::new(r"See merge request \S*!(\d+)").unwrap(),
    ]
});

/// The pull request number a commit subject carries: GitHub's merge commit
/// or squash suffix, Bitbucket's `(pull request #N)`, GitLab's merge request.
pub fn pull_request_number(subject: &str) -> Option<u64> {
    MARKERS
        .iter()
        .find_map(|re| re.captures(subject)?.get(1)?.as_str().parse().ok())
}

/// First-parent commits on HEAD, newest first, at most `limit`.
pub fn first_parent_log(root: &Path, limit: usize) -> Result<Vec<Commit>, GitError> {
    let text = run(
        root,
        &[
            "log",
            "--first-parent",
            "-n",
            &limit.to_string(),
            "--format=%H%x1f%h%x1f%P%x1f%cs%x1f%s%x1e",
            "HEAD",
        ],
    )?;
    Ok(text
        .split('\x1e')
        .filter_map(|record| {
            let fields: Vec<&str> = record.trim_start_matches('\n').split('\x1f').collect();
            if fields.len() < 5 || fields[0].is_empty() {
                return None;
            }
            let subject = fields[4].trim().to_string();
            Some(Commit {
                hash: fields[0].to_string(),
                short: fields[1].to_string(),
                parents: fields[2].split_whitespace().count(),
                date: fields[3].to_string(),
                pull_request: pull_request_number(&subject),
                subject,
            })
        })
        .collect())
}

fn lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// Files a commit changed against its first parent; a root commit against
/// the empty tree. Paths are relative to the repository top.
pub fn files_in_commit(root: &Path, commit: &Commit) -> Result<Vec<String>, GitError> {
    let text = if commit.parents == 0 {
        run(
            root,
            &[
                "diff-tree",
                "--root",
                "--no-commit-id",
                "--name-only",
                "-r",
                &commit.hash,
            ],
        )?
    } else {
        run(
            root,
            &[
                "diff",
                "--name-only",
                &format!("{}^1", commit.hash),
                &commit.hash,
            ],
        )?
    };
    Ok(lines(&text))
}

/// `git diff --name-only <range>`, verbatim.
pub fn files_in_range(root: &Path, range: &str) -> Result<Vec<String>, GitError> {
    let mut args = vec!["diff", "--name-only"];
    args.extend(range.split_whitespace());
    Ok(lines(&run(root, &args)?))
}

/// The change a diff range describes.
pub fn diff(root: &Path, range: &str) -> Result<Change, GitError> {
    let prefix = prefix(root)?;
    let (files, outside_root) = relative(&prefix, files_in_range(root, range)?);
    Ok(Change {
        kind: ChangeKind::Diff,
        reference: range.to_string(),
        how: None,
        title: None,
        date: None,
        files,
        outside_root,
    })
}

/// The change one commit made.
pub fn commit_change(root: &Path, prefix: &str, commit: &Commit) -> Result<Change, GitError> {
    let (files, outside_root) = relative(prefix, files_in_commit(root, commit)?);
    let (kind, reference) = match commit.pull_request {
        Some(n) => (ChangeKind::Pr, format!("#{n}")),
        None => (ChangeKind::Commit, commit.short.clone()),
    };
    let how = if commit.parents > 1 {
        format!("merge commit {}", commit.short)
    } else {
        format!("commit {}", commit.short)
    };
    Ok(Change {
        kind,
        reference,
        how: Some(how),
        title: Some(commit.subject.clone()),
        date: Some(commit.date.clone()),
        files,
        outside_root,
    })
}

/// Refs a fetched pull request may live under, in the order they are tried.
pub fn candidate_refs(number: u64) -> [String; 6] {
    [
        format!("refs/pull/{number}/head"),
        format!("refs/pull/{number}/merge"),
        format!("refs/remotes/origin/pr/{number}"),
        format!("refs/remotes/origin/pull/{number}/head"),
        format!("refs/heads/pr-{number}"),
        format!("refs/heads/pull/{number}"),
    ]
}

/// The change pull request `number` made: a first-parent commit carrying its
/// number, else a ref holding its head diffed from its merge base with HEAD.
pub fn pull_request(root: &Path, number: u64) -> Result<Change, GitError> {
    let prefix = prefix(root)?;
    let log = first_parent_log(root, LOG_LIMIT)?;
    if let Some(commit) = log.iter().find(|c| c.pull_request == Some(number)) {
        return commit_change(root, &prefix, commit);
    }
    for candidate in candidate_refs(number) {
        let Ok(hash) = run(root, &["rev-parse", "--verify", "--quiet", &candidate]) else {
            continue;
        };
        let hash = hash.trim().to_string();
        if hash.is_empty() {
            continue;
        }
        let base = run(root, &["merge-base", "HEAD", &hash])?
            .trim()
            .to_string();
        let files = lines(&run(root, &["diff", "--name-only", &base, &hash])?);
        let (files, outside_root) = relative(&prefix, files);
        let meta = run(root, &["log", "-1", "--format=%h%x1f%cs%x1f%s", &hash])?;
        let fields: Vec<&str> = meta.trim().split('\x1f').collect();
        let short = fields.first().copied().unwrap_or("").to_string();
        return Ok(Change {
            kind: ChangeKind::Pr,
            reference: format!("#{number}"),
            how: Some(format!("ref {candidate} at {short}")),
            title: fields.get(2).map(|s| s.trim().to_string()),
            date: fields.get(1).map(ToString::to_string),
            files,
            outside_root,
        });
    }
    Err(GitError::PullRequestNotFound {
        number,
        root: root.display().to_string(),
        searched: log.len(),
    })
}

/// The recent changes `--history` walks: first-parent commits carrying a
/// pull request number, newest first, until `n`; or, when no commit carries
/// one, the last `n` first-parent commits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recent {
    pub commits: Vec<Commit>,
    pub pull_requests: bool,
}

pub fn recent(root: &Path, n: usize) -> Result<Recent, GitError> {
    let log = first_parent_log(root, LOG_LIMIT)?;
    let prs: Vec<Commit> = log
        .iter()
        .filter(|c| c.pull_request.is_some())
        .take(n)
        .cloned()
        .collect();
    if prs.is_empty() {
        return Ok(Recent {
            commits: log.into_iter().take(n).collect(),
            pull_requests: false,
        });
    }
    Ok(Recent {
        commits: prs,
        pull_requests: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn subjects_carry_pull_request_numbers_in_four_shapes() {
        assert_eq!(
            pull_request_number("Merge pull request #481 from acme/checkout-retry"),
            Some(481)
        );
        assert_eq!(
            pull_request_number("feat(map): import edges (#12)"),
            Some(12)
        );
        assert_eq!(
            pull_request_number("Merged in feature/x (pull request #77)"),
            Some(77)
        );
        assert_eq!(
            pull_request_number(
                "Merge branch 'x' into 'main'\n\nSee merge request acme/platform!903"
            ),
            Some(903)
        );
        assert_eq!(pull_request_number("fix: issue #12 in the parser"), None);
        assert_eq!(pull_request_number("Merge branch 'feature'"), None);
        assert_eq!(pull_request_number("(#12) at the start"), None);
    }

    #[test]
    fn relative_strips_the_prefix_and_counts_the_rest() {
        let paths = vec![
            "sub/dir/a.js".to_string(),
            "sub/dir/b/c.py".to_string(),
            "other/x".to_string(),
            "sub/dir/".to_string(),
        ];
        assert_eq!(
            relative("sub/dir/", paths.clone()),
            (vec!["a.js".to_string(), "b/c.py".to_string()], 2)
        );
        assert_eq!(relative("", paths.clone()), (paths, 0));
    }

    #[test]
    fn candidate_refs_cover_github_and_local_conventions() {
        let refs = candidate_refs(9);
        assert_eq!(refs[0], "refs/pull/9/head");
        assert!(refs.iter().any(|r| r == "refs/heads/pr-9"));
    }

    #[test]
    fn a_directory_outside_any_repository_is_reported() {
        let root = std::env::temp_dir().join(format!("vernier-git-none-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let err = first_parent_log(&root, 5).unwrap_err();
        assert!(matches!(err, GitError::NotARepository(_)), "{err}");
        assert!(!is_repository(&root));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
