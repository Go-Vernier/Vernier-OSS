//! `--history N`: the blast radius of each recent pull request, and the
//! numbers across them. Field names are the JSON contract.
use serde::{Deserialize, Serialize};

use crate::analyze::Analysis;
use crate::blast;
use crate::git::{self, GitError};

/// What the history counts. Commits, when the log carries no pull request
/// markers, and the report says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Unit {
    #[serde(rename = "pull requests")]
    PullRequests,
    #[serde(rename = "commits")]
    Commits,
}

impl Unit {
    /// `PRs` or `commits`, singular when `n` is one.
    pub fn noun(self, n: usize) -> &'static str {
        match (self, n) {
            (Self::PullRequests, 1) => "PR",
            (Self::PullRequests, _) => "PRs",
            (Self::Commits, 1) => "commit",
            (Self::Commits, _) => "commits",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// `#481`, or the short hash when the unit is commits.
    pub reference: String,
    pub commit: String,
    pub date: String,
    pub title: String,
    /// Files the change touched inside the analysed root.
    pub files: usize,
    /// Services those files belong to.
    pub changed: usize,
    /// Code services the change reaches; its own are not counted.
    pub reached: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Largest {
    pub reference: String,
    pub reached: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Over10 {
    pub count: usize,
    /// Of the entries found, rounded.
    pub percent: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct History {
    pub requested: usize,
    pub found: usize,
    pub unit: Unit,
    pub depth: usize,
    /// Newest first.
    pub entries: Vec<Entry>,
    /// Mean reached, to one decimal.
    pub average: f64,
    pub median: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub largest: Option<Largest>,
    pub over_10: Over10,
    /// Entries whose files belong to no service (documentation, CI, ...).
    pub touching_no_service: usize,
}

/// Runs the walk once per recent change and aggregates.
pub fn run(analysis: &Analysis, n: usize, depth: usize) -> Result<History, GitError> {
    let recent = git::recent(&analysis.root, n)?;
    let prefix = git::prefix(&analysis.root)?;
    let mut entries: Vec<Entry> = Vec::new();
    for commit in &recent.commits {
        let mut change = git::commit_change(&analysis.root, &prefix, commit)?;
        if !recent.pull_requests {
            change.kind = blast::ChangeKind::Commit;
            change.reference.clone_from(&commit.short);
        }
        let b = blast::of_change(&analysis.graph, change, depth);
        entries.push(Entry {
            reference: b.change.reference.clone(),
            commit: commit.short.clone(),
            date: commit.date.clone(),
            title: commit.subject.clone(),
            files: b.change.files.len(),
            changed: b.summary.changed,
            reached: b.summary.reached,
        });
    }
    Ok(summarise(
        n,
        if recent.pull_requests {
            Unit::PullRequests
        } else {
            Unit::Commits
        },
        depth,
        entries,
    ))
}

/// The numbers over the entries. Separate from `run` so it can be tested
/// without a repository.
#[allow(clippy::cast_precision_loss)]
pub fn summarise(requested: usize, unit: Unit, depth: usize, entries: Vec<Entry>) -> History {
    let found = entries.len();
    let mut reached: Vec<usize> = entries.iter().map(|e| e.reached).collect();
    reached.sort_unstable();
    let average = if found == 0 {
        0.0
    } else {
        round1(reached.iter().sum::<usize>() as f64 / found as f64)
    };
    let median = match found {
        0 => 0.0,
        n if n % 2 == 1 => reached[n / 2] as f64,
        n => round1((reached[n / 2 - 1] + reached[n / 2]) as f64 / 2.0),
    };
    let largest = entries
        .iter()
        .max_by(|a, b| a.reached.cmp(&b.reached))
        .map(|e| Largest {
            reference: e.reference.clone(),
            reached: e.reached,
        });
    let count = entries.iter().filter(|e| e.reached > 10).count();
    let percent = (count * 100 + found / 2)
        .checked_div(found)
        .and_then(|p| u32::try_from(p).ok())
        .unwrap_or(0);
    History {
        requested,
        found,
        unit,
        depth,
        touching_no_service: entries.iter().filter(|e| e.changed == 0).count(),
        entries,
        average,
        median,
        largest,
        over_10: Over10 { count, percent },
    }
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn entry(reference: &str, changed: usize, reached: usize) -> Entry {
        Entry {
            reference: reference.into(),
            commit: "abc1234".into(),
            date: "2026-09-10".into(),
            title: format!("change {reference}"),
            files: 3,
            changed,
            reached,
        }
    }

    #[test]
    fn summarise_computes_mean_median_largest_and_over_ten() {
        let h = summarise(
            50,
            Unit::PullRequests,
            3,
            vec![
                entry("#5", 1, 3),
                entry("#4", 0, 0),
                entry("#3", 2, 31),
                entry("#2", 1, 12),
                entry("#1", 1, 4),
            ],
        );
        assert_eq!((h.requested, h.found), (50, 5));
        assert_eq!(h.average, 10.0);
        assert_eq!(h.median, 4.0);
        assert_eq!(
            h.largest,
            Some(Largest {
                reference: "#3".into(),
                reached: 31
            })
        );
        assert_eq!(
            h.over_10,
            Over10 {
                count: 2,
                percent: 40
            }
        );
        assert_eq!(h.touching_no_service, 1);
        let json = serde_json::to_value(&h).unwrap();
        assert_eq!(json["unit"], "pull requests");
        assert_eq!(
            json["over10"],
            serde_json::json!({ "count": 2, "percent": 40 })
        );
        assert_eq!(json["entries"][0]["reference"], "#5");
        assert!(json["touchingNoService"].is_number());
    }

    #[test]
    fn even_counts_take_the_middle_pair_and_empty_is_zero() {
        let h = summarise(
            2,
            Unit::Commits,
            3,
            vec![entry("a1", 1, 2), entry("b2", 1, 5)],
        );
        assert_eq!(h.median, 3.5);
        assert_eq!(h.average, 3.5);
        assert_eq!(Unit::Commits.noun(2), "commits");
        assert_eq!(Unit::PullRequests.noun(1), "PR");
        let empty = summarise(10, Unit::Commits, 3, vec![]);
        assert_eq!((empty.average, empty.median), (0.0, 0.0));
        assert_eq!(empty.largest, None);
        assert_eq!(serde_json::to_value(&empty).unwrap().get("largest"), None);
    }
}
