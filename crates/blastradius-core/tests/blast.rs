mod common;

use blastradius::blast::{self, ByConfidence, ChangeKind, Relation, Summary};
use blastradius::history::Unit;
use blastradius::*;
use common::{fixture, http_repo, nested_repo};
use pretty_assertions::assert_eq;

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_string()).collect()
}

fn radius_of(name: &str, files: &[&str], depth: usize) -> Blast {
    let a = analyze(&fixture(name)).unwrap();
    blast::of_change(&a.graph, Change::from_files(&strings(files)), depth)
}

fn names(b: &Blast) -> Vec<(&str, usize, Confidence)> {
    b.reached
        .iter()
        .map(|r| (r.service.as_str(), r.depth, r.confidence))
        .collect()
}

#[test]
fn http_calls_reach_callers_and_their_callers() {
    let b = radius_of("edges-http-app", &["catalogue/main.go", "README.md"], 3);
    assert_eq!(b.changed.len(), 1);
    assert_eq!(b.changed[0].service, "catalogue");
    assert_eq!(b.unowned, vec!["README.md"]);
    assert_eq!(
        names(&b),
        vec![
            ("cart", 1, Confidence::Static),
            ("ratings", 1, Confidence::Static),
            ("web", 1, Confidence::Static),
            ("payment", 2, Confidence::Static),
        ]
    );
    let payment = &b.reached[3];
    assert_eq!(payment.path.len(), 2);
    assert_eq!(payment.path[0].relation, Relation::Calls);
    assert_eq!(
        (payment.path[0].from.as_str(), payment.path[0].to.as_str()),
        ("catalogue", "cart")
    );
    assert_eq!(
        (payment.path[1].from.as_str(), payment.path[1].to.as_str()),
        ("cart", "payment")
    );
    assert_eq!(b.not_reached, Vec::<String>::new());
    assert_eq!(
        b.summary,
        Summary {
            changed: 1,
            reached: 4,
            by_confidence: ByConfidence {
                observed: 0,
                static_: 4,
                inferred: 0,
                uncertain: 0
            }
        }
    );

    let shallow = radius_of("edges-http-app", &["catalogue/main.go"], 1);
    assert!(shallow.reached.iter().all(|r| r.depth == 1));
    assert_eq!(shallow.not_reached, vec!["payment"]);
}

#[test]
fn event_consumers_are_reached_and_the_broker_step_is_uncertain() {
    let b = radius_of("edges-events-app", &["checkout/producer.js"], 3);
    assert_eq!(
        names(&b),
        vec![
            ("accounting", 1, Confidence::Inferred),
            ("notifications", 1, Confidence::Inferred),
            ("webhooks", 2, Confidence::Inferred),
        ],
        "{:#?}",
        b.reached
    );
    assert_eq!(b.reached[0].path[0].relation, Relation::Consumes);
    assert_eq!(b.reached[2].path[1].from, "accounting");
    // kafka's other clients were already reached through their topics
    assert_eq!(b.infrastructure.len(), 1);
    assert_eq!(b.infrastructure[0].service, "kafka");
    assert_eq!(b.not_reached, vec!["dispatch", "payment"]);

    // dispatch only talks to rabbitmq: its other clients are reached through
    // the broker, as uncertain, and no further
    let b = radius_of("edges-events-app", &["dispatch/main.go"], 3);
    assert_eq!(
        names(&b),
        vec![
            ("notifications", 1, Confidence::Uncertain),
            ("payment", 1, Confidence::Uncertain),
        ]
    );
    assert_eq!(b.reached[0].path[0].relation, Relation::SharesBroker);
    assert_eq!(b.reached[0].path[0].via.as_deref(), Some("rabbitmq"));
    assert_eq!(
        b.infrastructure,
        vec![Touched {
            service: "rabbitmq".into(),
            via: "dispatch".into()
        }]
    );
    assert_eq!(b.not_reached, vec!["accounting", "checkout", "webhooks"]);
}

#[test]
fn shared_databases_and_imports_are_followed() {
    let b = radius_of("edges-db-app", &["ledger/Program.cs"], 3);
    assert_eq!(names(&b), vec![("audit", 1, Confidence::Inferred)]);
    assert_eq!(b.reached[0].path[0].relation, Relation::SharesDatabase);
    // postgres is a dependency of ledger's neighbours, never "reached"
    assert!(b.reached.iter().all(|r| r.service != "postgres"));

    let b = radius_of("edges-import-app", &["packages/shared/package.json"], 3);
    assert_eq!(names(&b), vec![("web", 1, Confidence::Static)]);
    assert_eq!(b.reached[0].path[0].relation, Relation::Imports);
    assert_eq!(b.reached[0].path[0].edge_type, EdgeType::Import);
}

#[test]
fn observed_edges_carry_their_call_counts_into_the_path() {
    let root = fixture("runtime-app");
    let mut a = analyze(&root).unwrap();
    let text = std::fs::read_to_string(root.join("runtime/traces.prom")).unwrap();
    let graph = runtime::prometheus::parse(&text, "traces.prom").unwrap();
    runtime::join(&mut a, graph, &config::load(&root).unwrap().runtime).unwrap();
    a.blast = Some(blast::of_change(
        &a.graph,
        Change::from_files(&strings(&["payment/payment.py"])),
        3,
    ));
    let b = a.blast.as_ref().unwrap();
    assert_eq!(names(b), vec![("checkout", 1, Confidence::Observed)]);
    assert_eq!(b.reached[0].path[0].calls, Some(132));
    let r = format_report(&a, false);
    assert!(
        r.contains("checkout  1      observed    checkout calls payment (http, 132 calls)"),
        "{r}"
    );
    assert!(
        r.contains("RUNTIME") && r.contains("8 of 10 runtime services matched"),
        "{r}"
    );
}

#[test]
fn change_report_has_the_headline_and_the_fixed_wording() {
    let mut a = analyze(&fixture("edges-http-app")).unwrap();
    a.blast = Some(blast::of_change(
        &a.graph,
        Change::from_files(&strings(&["catalogue/main.go", "README.md"])),
        3,
    ));
    let r = format_report(&a, false);
    assert!(r.contains("  Change        2 files given"), "{r}");
    assert!(r.contains("2 files in 1 service, 1 in no service"), "{r}");
    assert!(r.contains("BLAST RADIUS"), "{r}");
    assert!(
        r.contains("1 service changed -> 4 services in the blast radius"),
        "{r}"
    );
    assert!(r.contains("4 static · depth 3"), "{r}");
    assert!(r.contains("CHANGED") && r.contains("REACHED"), "{r}");
    assert!(
        regex::Regex::new(
            r"payment\s+2\s+static\s+payment calls cart \(http\); cart calls catalogue \(http\)"
        )
        .unwrap()
        .is_match(&r),
        "{r}"
    );
    assert!(
        r.contains("1 changed file belongs to no service: README.md"),
        "{r}"
    );
    assert!(r.contains(blast::NOT_REACHED), "{r}");
    assert!(
        r.contains("0 services  (every other service is in the computed blast radius)"),
        "{r}"
    );
    assert!(
        !r.contains("SERVICES\n") && !r.contains("EDGES\n"),
        "the change report does not repeat the repository report: {r}"
    );
    assert!(!r.contains('\u{1b}'));

    // a change that touches nothing the tool knows
    a.blast = Some(blast::of_change(
        &a.graph,
        Change::from_files(&strings(&["docs/adr/0007.md"])),
        3,
    ));
    let r = format_report(&a, false);
    assert!(
        r.contains("0 services changed -> 0 services in the blast radius"),
        "{r}"
    );
    assert!(
        r.contains("None of the 1 changed file belongs to a discovered service: docs/adr/0007.md"),
        "{r}"
    );
    assert!(r.contains(blast::NOT_REACHED), "{r}");
    assert!(
        regex::Regex::new(r"5 services\s+cart, catalogue, payment, ratings, web")
            .unwrap()
            .is_match(&r),
        "{r}"
    );

    // a service walked through a broker is dimmed but present
    let mut a = analyze(&fixture("edges-events-app")).unwrap();
    a.blast = Some(blast::of_change(
        &a.graph,
        Change::from_files(&strings(&["dispatch/main.go"])),
        3,
    ));
    let r = format_report(&a, false);
    assert!(
        r.contains("notifications shares broker rabbitmq with dispatch"),
        "{r}"
    );
    assert!(
        r.contains("Infrastructure on the path   rabbitmq (published to by dispatch)"),
        "{r}"
    );
    assert!(r.contains("2 uncertain · depth 3"), "{r}");
}

#[test]
fn repository_report_names_the_widest_change_surface() {
    let r = format_repo_report(&analyze(&fixture("edges-http-app")).unwrap(), false);
    assert!(
        regex::Regex::new(
            r"Widest change surface\s+catalogue\n\s+a change here reaches 4 services"
        )
        .unwrap()
        .is_match(&r),
        "{r}"
    );
    let r = format_repo_report(&analyze(&fixture("edges-import-app")).unwrap(), false);
    assert!(
        regex::Regex::new(r"Widest change surface\s+\w")
            .unwrap()
            .is_match(&r),
        "{r}"
    );
    assert_eq!(
        blast::widest(&analyze(&fixture("edges-http-app")).unwrap().graph, 1),
        Some(("catalogue".into(), 3))
    );
}

#[test]
fn json_contract_gains_blast_and_history_only_when_asked() {
    let mut a = analyze(&fixture("edges-http-app")).unwrap();
    let json = serde_json::to_value(a.to_json()).unwrap();
    let keys: Vec<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        vec![
            "repository",
            "root",
            "discovery",
            "services",
            "edges",
            "mapping",
            "runtime"
        ]
    );

    a.blast = Some(blast::of_change(
        &a.graph,
        Change::from_files(&strings(&["catalogue/main.go"])),
        2,
    ));
    let json = serde_json::to_value(a.to_json()).unwrap();
    let keys: Vec<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys.last(), Some(&"blast"));
    let b = &json["blast"];
    let keys: Vec<&str> = b.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        vec![
            "change",
            "depth",
            "changed",
            "unowned",
            "reached",
            "infrastructure",
            "notReached",
            "summary"
        ]
    );
    assert_eq!(
        b["change"],
        serde_json::json!({ "kind": "files", "reference": "1 file", "files": ["catalogue/main.go"], "outsideRoot": 0 })
    );
    assert_eq!(b["depth"], 2);
    assert_eq!(b["reached"][0]["service"], "cart");
    assert_eq!(
        b["reached"][0]["path"][0],
        serde_json::json!({ "from": "catalogue", "to": "cart", "relation": "calls", "type": "http", "confidence": "static" })
    );
    assert_eq!(b["summary"]["reached"], 4);
    let back: AnalysisJson = serde_json::from_value(json).unwrap();
    assert_eq!(back.blast.unwrap().summary.reached, 4);
}

#[test]
#[allow(clippy::too_many_lines)]
fn pull_requests_diffs_and_recent_history_come_from_the_local_repository() {
    let repo = http_repo("git-http");
    let root = &repo.root;

    let pr7 = git::pull_request(root, 7).unwrap();
    assert_eq!(pr7.kind, ChangeKind::Pr);
    assert_eq!(pr7.reference, "#7");
    assert!(
        pr7.how.as_deref().unwrap().starts_with("merge commit "),
        "{:?}",
        pr7.how
    );
    assert_eq!(
        pr7.title.as_deref(),
        Some("Merge pull request #7 from acme/catalogue-handler")
    );
    assert_eq!(pr7.date.as_deref().map(str::len), Some(10));
    assert_eq!(pr7.files, vec!["catalogue/handlers.go"]);
    assert_eq!(pr7.outside_root, 0);

    let pr8 = git::pull_request(root, 8).unwrap();
    assert!(
        pr8.how.as_deref().unwrap().starts_with("commit "),
        "{:?}",
        pr8.how
    );
    assert_eq!(pr8.files, vec!["README.md", "web/default.conf.template"]);

    let pr9 = git::pull_request(root, 9).unwrap();
    assert!(
        pr9.how
            .as_deref()
            .unwrap()
            .starts_with("ref refs/pull/9/head at "),
        "{:?}",
        pr9.how
    );
    assert_eq!(pr9.title.as_deref(), Some("cart: coupon"));
    assert_eq!(pr9.files, vec!["cart/server.js"]);

    let err = git::pull_request(root, 10).unwrap_err();
    assert!(
        matches!(err, GitError::PullRequestNotFound { number: 10, .. }),
        "{err}"
    );
    assert!(
        err.to_string()
            .contains("git fetch origin pull/10/head:refs/pull/10/head"),
        "{err}"
    );
    assert!(
        err.to_string().contains("--diff") && err.to_string().contains("--files"),
        "{err}"
    );

    let diff = git::diff(root, "HEAD~1").unwrap();
    assert_eq!(diff.kind, ChangeKind::Diff);
    assert_eq!(diff.reference, "HEAD~1");
    assert_eq!(diff.files, vec!["payment/payment.py"]);
    let diff = git::diff(root, "HEAD~3 HEAD~1").unwrap();
    assert_eq!(
        diff.files,
        vec!["README.md", "docs/notes.md", "web/default.conf.template"]
    );

    let recent = git::recent(root, 10).unwrap();
    assert!(recent.pull_requests);
    assert_eq!(
        recent
            .commits
            .iter()
            .map(|c| c.pull_request)
            .collect::<Vec<_>>(),
        vec![Some(8), Some(7)]
    );
    assert_eq!(recent.commits[1].parents, 2);
    assert_eq!(git::recent(root, 1).unwrap().commits.len(), 1);
    assert!(git::is_repository(root));
    assert_eq!(git::prefix(root).unwrap(), "");

    // the whole thing through the analysis
    let analysis = analyze(root).unwrap();
    let h = history::run(&analysis, 10, 3).unwrap();
    assert_eq!((h.requested, h.found, h.unit), (10, 2, Unit::PullRequests));
    assert_eq!(h.entries[0].reference, "#8");
    assert_eq!(
        (
            h.entries[0].files,
            h.entries[0].changed,
            h.entries[0].reached
        ),
        (2, 1, 0)
    );
    assert_eq!(h.entries[1].reference, "#7");
    assert_eq!(
        (
            h.entries[1].files,
            h.entries[1].changed,
            h.entries[1].reached
        ),
        (1, 1, 4)
    );
    assert_eq!((h.average, h.median), (2.0, 2.0));
    assert_eq!(
        h.largest
            .as_ref()
            .map(|l| (l.reference.as_str(), l.reached)),
        Some(("#7", 4))
    );
    assert_eq!(h.over_10.count, 0);
    assert_eq!(h.touching_no_service, 0);

    let mut analysis = analysis;
    analysis.history = Some(h);
    let r = format_repo_report(&analysis, false);
    assert!(
        r.contains("CHANGE HISTORY  (last 2 PRs, 10 asked for)"),
        "{r}"
    );
    assert!(
        regex::Regex::new(r"Average blast radius\s+2\.0 services")
            .unwrap()
            .is_match(&r),
        "{r}"
    );
    assert!(
        regex::Regex::new(r"Median\s+2\n").unwrap().is_match(&r),
        "{r}"
    );
    assert!(
        regex::Regex::new(r"Largest\s+PR #7 - 4 services")
            .unwrap()
            .is_match(&r),
        "{r}"
    );
    assert!(
        regex::Regex::new(r"PRs reaching >10\s+0  \(0%\)")
            .unwrap()
            .is_match(&r),
        "{r}"
    );
    assert!(
        regex::Regex::new(r"#8\s+\d{4}-\d{2}-\d{2}\s+2\s+1\s+0\s+web: tweak nginx \(#8\)")
            .unwrap()
            .is_match(&r),
        "{r}"
    );
    assert!(!r.contains("No pull request markers"), "{r}");

    // a change report and the history together
    analysis.blast = Some(blast::of_change(
        &analysis.graph,
        git::pull_request(root, 7).unwrap(),
        3,
    ));
    let r = format_report(&analysis, false);
    assert!(
        regex::Regex::new(r"Change        PR #7  merge commit [0-9a-f]+  \d{4}-\d{2}-\d{2}")
            .unwrap()
            .is_match(&r),
        "{r}"
    );
    assert!(
        r.contains("Merge pull request #7 from acme/catalogue-handler"),
        "{r}"
    );
    assert!(
        r.contains("1 service changed -> 4 services in the blast radius"),
        "{r}"
    );
    assert!(r.contains("CHANGE HISTORY"), "{r}");
    let json = serde_json::to_value(analysis.to_json()).unwrap();
    assert_eq!(json["blast"]["change"]["kind"], "pr");
    assert_eq!(json["history"]["unit"], "pull requests");
    assert_eq!(json["history"]["found"], 2);
}

#[test]
fn a_nested_root_and_a_history_without_pull_request_markers() {
    let repo = nested_repo("git-nested");
    let app = repo.root.join("nested/app");
    assert_eq!(git::prefix(&app).unwrap(), "nested/app/");
    let diff = git::diff(&app, "HEAD~1").unwrap();
    assert_eq!(diff.files, vec!["checkout/producer.js"]);
    assert_eq!(diff.outside_root, 1);

    let analysis = analyze(&app).unwrap();
    let h = history::run(&analysis, 5, 3).unwrap();
    assert_eq!((h.found, h.unit), (2, Unit::Commits));
    assert_eq!(h.entries[0].title, "checkout: publish v2 events");
    assert_eq!((h.entries[0].changed, h.entries[0].reached), (1, 3));
    assert_eq!(h.entries[1].title, "initial");
    assert!(
        h.entries[1].changed >= 6,
        "the initial commit touches every service: {:?}",
        h.entries[1]
    );
    assert_eq!(h.entries[0].reference.len(), h.entries[0].commit.len());

    let mut analysis = analysis;
    analysis.history = Some(h);
    let r = format_repo_report(&analysis, false);
    assert!(
        r.contains("CHANGE HISTORY  (last 2 commits, 5 asked for)"),
        "{r}"
    );
    assert!(
        r.contains(
            "No pull request markers in the history; each first-parent commit counts as one change."
        ),
        "{r}"
    );
    assert!(
        regex::Regex::new(r"Largest\s+commit [0-9a-f]+ - ")
            .unwrap()
            .is_match(&r),
        "{r}"
    );
    assert!(r.contains("Commits reaching >10"), "{r}");

    let mut a = analyze(&app).unwrap();
    a.blast = Some(blast::of_change(&a.graph, diff, 3));
    let r = format_report(&a, false);
    assert!(r.contains("Change        diff HEAD~1"), "{r}");
    assert!(
        r.contains("1 file in 1 service, 1 outside the analysed directory"),
        "{r}"
    );
}

#[test]
fn html_report_is_self_contained_and_embeds_the_contract() {
    let mut a = analyze(&fixture("edges-http-app")).unwrap();
    let plain = html::render(&a);
    assert!(plain.starts_with("<!doctype html>"));
    assert!(plain.contains("<title>Vernier · "), "{}", &plain[..200]);
    assert!(plain.contains(r#"<script id="vernier-data" type="application/json">"#));
    assert!(
        !plain.contains("src=\"http")
            && !plain.contains("href=\"http")
            && !plain.contains("@import"),
        "no external resource"
    );
    assert!(
        plain.contains("\"widest\":{\"service\":\"catalogue\",\"reached\":4}"),
        "findings travel with the data"
    );
    a.blast = Some(blast::of_change(
        &a.graph,
        Change::from_files(&["catalogue/main.go".to_string()]),
        3,
    ));
    let with = html::render(&a);
    assert!(
        with.contains("\"blast\":{\"change\""),
        "the blast block is embedded"
    );
    assert!(
        with.contains(&html::escape_json(blast::NOT_REACHED)) || with.contains(blast::NOT_REACHED)
    );
    let data_start = with.find(r#"<script id="vernier-data""#).unwrap();
    let data_end = with[data_start..].find("</script>").unwrap() + data_start;
    let inner = &with[data_start..data_end];
    let json_start = inner.find('>').unwrap() + 1;
    let json: serde_json::Value = serde_json::from_str(&inner[json_start..]).unwrap();
    assert_eq!(json["analysis"]["blast"]["summary"]["reached"], 4);
}
