//! The TUI against real analyses: keys in, state out, and every tab drawn
//! on a test backend and compared to a text snapshot. `UPDATE_SNAPSHOTS=1`
//! rewrites the snapshots.
use std::path::{Path, PathBuf};
use std::process::Command;

use blastradius::{Analysis, ChangeKind, blast};
use blastradius_tui::app::{Pane, Tab};
use blastradius_tui::{Action, App, Options};
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

// ------------------------------------------------------------- repositories

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures")
        .join(name)
}

/// A throwaway repository under the system temp directory, removed on drop.
struct TempRepo {
    root: PathBuf,
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Every commit gets the same date and author, so the Changes tab draws the
/// same on every machine.
fn git(root: &Path, args: &[&str]) {
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
        .env("GIT_AUTHOR_DATE", "2026-09-01T12:00:00Z")
        .env("GIT_COMMITTER_DATE", "2026-09-01T12:00:00Z")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn copy_dir(from: &Path, to: &Path) {
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

fn append(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    let mut text = std::fs::read_to_string(&path).unwrap_or_default();
    text.push_str(content);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// `fixture` as a repository named `acme/shop` whose history holds, newest
/// first: squash-merged PR #8 on web, merged PR #7 on catalogue, and the
/// initial commit.
fn repo(fixture_name: &str, tag: &str) -> TempRepo {
    let root = std::env::temp_dir().join(format!("vernier-tui-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    copy_dir(&fixture(fixture_name), &root);
    git(&root, &["init", "-q", "-b", "main"]);
    git(
        &root,
        &["remote", "add", "origin", "git@github.com:acme/shop.git"],
    );
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "initial"]);
    TempRepo { root }
}

fn http_repo(tag: &str) -> TempRepo {
    let repo = repo("edges-http-app", tag);
    let root = &repo.root;
    git(root, &["checkout", "-q", "-b", "catalogue-handler"]);
    append(root, "catalogue/handlers.go", "package main\n\n// pr 7\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "catalogue: add handler"]);
    git(root, &["checkout", "-q", "main"]);
    git(
        root,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "Merge pull request #7 from acme/catalogue-handler",
            "catalogue-handler",
        ],
    );
    append(root, "web/default.conf.template", "# pr 8\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "web: tweak nginx (#8)"]);
    repo
}

fn analysis(root: &Path) -> Analysis {
    blastradius::analyze(root).unwrap()
}

fn app(root: &Path, history: usize) -> App {
    App::new(
        analysis(root),
        Options {
            depth: blast::DEFAULT_DEPTH,
            history,
            color: false,
            change: None,
        },
    )
}

// --------------------------------------------------------------------- keys

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// Types `keys` one character at a time.
fn press(app: &mut App, keys: &str) {
    for c in keys.chars() {
        app.update(key(KeyCode::Char(c)));
    }
}

// ---------------------------------------------------------------- rendering

fn render(app: &App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| blastradius_tui::ui::draw(frame, app))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let mut lines = Vec::new();
    for y in 0..buffer.area.height {
        let line: String = (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect();
        lines.push(line.trim_end().to_string());
    }
    lines.join("\n") + "\n"
}

fn snapshot(name: &str, actual: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(format!("{name}.txt"));
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "no snapshot {}; run with UPDATE_SNAPSHOTS=1\n{actual}",
            path.display()
        )
    });
    assert_eq!(expected, actual, "snapshot {name}");
}

// -------------------------------------------------------------------- tests

#[test]
fn every_tab_draws_the_same_as_its_snapshot() {
    let repo = http_repo("snapshots");
    let mut app = app(&repo.root, 50);
    snapshot("overview", &render(&app, 100, 30));

    press(&mut app, "2");
    snapshot("services", &render(&app, 100, 30));
    press(&mut app, "jjl");
    assert_eq!(app.pane, Pane::Edges);
    snapshot("services-edge", &render(&app, 100, 30));

    press(&mut app, "3");
    snapshot("changes", &render(&app, 100, 30));

    press(&mut app, "j");
    app.update(key(KeyCode::Enter));
    assert_eq!(app.tab, Tab::Blast);
    snapshot("blast-pr", &render(&app, 100, 30));
    press(&mut app, "G");
    snapshot("blast-pr-last", &render(&app, 100, 30));

    press(&mut app, "?");
    snapshot("help", &render(&app, 100, 30));
}

#[test]
fn a_blank_blast_tab_says_how_to_walk_a_change() {
    let repo = http_repo("blank");
    let mut app = app(&repo.root, 50);
    press(&mut app, "4");
    let text = render(&app, 100, 30);
    assert!(text.contains("No change walked yet."), "{text}");
}

#[test]
fn enter_on_a_service_walks_a_change_to_its_root() {
    let repo = http_repo("service");
    let mut app = app(&repo.root, 0);
    press(&mut app, "2/catalogue");
    app.update(key(KeyCode::Enter));
    assert!(!app.filtering);
    assert_eq!(app.filtered_services().len(), 1);
    app.update(key(KeyCode::Enter));
    assert_eq!(app.tab, Tab::Blast);
    let b = app.blast.as_ref().unwrap();
    assert_eq!(b.change.reference, "service catalogue");
    assert_eq!(b.change.kind, ChangeKind::Files);
    assert_eq!(b.changed.len(), 1);
    assert_eq!(b.changed[0].service, "catalogue");
    let text = render(&app, 100, 30);
    assert!(
        text.contains("1 service changed -> ") && text.contains("Change  service catalogue"),
        "{text}"
    );

    // Esc goes back to where the walk was opened, with the filter kept.
    app.update(key(KeyCode::Esc));
    assert_eq!(app.tab, Tab::Services);
    assert_eq!(app.service_filter, "catalogue");
    app.update(key(KeyCode::Esc));
    assert_eq!(app.service_filter, "");
}

#[test]
fn depth_re_walks_the_change_and_keeps_the_selection() {
    let repo = http_repo("depth");
    let mut app = app(&repo.root, 0);
    press(&mut app, "2/catalogue");
    app.update(key(KeyCode::Enter));
    app.update(key(KeyCode::Enter));
    let at = |app: &App| app.blast.as_ref().unwrap().reached.len();
    let deep = at(&app);
    press(&mut app, "--");
    assert_eq!(app.depth, 1);
    assert_eq!(app.blast.as_ref().unwrap().depth, 1);
    assert!(at(&app) <= deep);
    press(&mut app, "-");
    assert_eq!(app.depth, 1);
    assert_eq!(app.status.as_deref(), Some("depth stays between 1 and 20"));
    press(&mut app, "+++");
    assert_eq!(app.depth, 4);
    assert!(at(&app) >= deep);

    // The cursor walks the tree; a shallower walk keeps it on the same
    // service while that service is still in the tree.
    press(&mut app, "g");
    assert_eq!(app.selected_row().unwrap().service, "catalogue");
    press(&mut app, "j");
    let name = app.selected_row().unwrap().service.clone();
    press(&mut app, "-");
    assert_eq!(app.selected_row().unwrap().service, name);
    press(&mut app, "G");
    assert_eq!(app.reached_sel, app.tree.len() - 1);
}

#[test]
fn the_walk_tree_hangs_every_reached_service_off_its_path() {
    let repo = http_repo("tree");
    let mut app = app(&repo.root, 0);
    press(&mut app, "2/catalogue");
    app.update(key(KeyCode::Enter));
    app.update(key(KeyCode::Enter));
    let b = app.blast.as_ref().unwrap();
    let rows = blastradius_tui::tree::rows(b);
    // One root per changed service, then one row per reached service at
    // least; a service on another's path may appear again as a step.
    assert_eq!(rows[0].kind, blastradius_tui::tree::Kind::Root);
    for (i, r) in b.reached.iter().enumerate() {
        let row = rows
            .iter()
            .find(|row| row.kind == blastradius_tui::tree::Kind::Reached(i))
            .unwrap_or_else(|| panic!("{} has no row", r.service));
        assert_eq!(row.service, r.service);
        // Indented one level per hop.
        assert_eq!(row.prefix.chars().count(), 4 * r.path.len());
    }
    // The cursor opens on the first reached service, not the root.
    assert!(matches!(
        app.selected_row().unwrap().kind,
        blastradius_tui::tree::Kind::Reached(_)
    ));
}

#[test]
fn the_prompt_walks_a_diff_range_or_a_list_of_files() {
    let repo = http_repo("prompt");
    let mut app = app(&repo.root, 50);
    press(&mut app, "3dHEAD~1");
    assert_eq!(app.prompt.as_ref().unwrap().text, "HEAD~1");
    app.update(key(KeyCode::Enter));
    let b = app.blast.as_ref().unwrap();
    assert_eq!(b.change.kind, ChangeKind::Diff);
    assert_eq!(b.change.files, vec!["web/default.conf.template"]);
    assert_eq!(app.opened_from, Tab::Changes);

    press(&mut app, "3fcart/server.js payment/payment.py");
    app.update(key(KeyCode::Enter));
    let b = app.blast.as_ref().unwrap();
    assert_eq!(b.change.kind, ChangeKind::Files);
    assert_eq!(b.changed.len(), 2);

    press(&mut app, "3dno-such-ref");
    app.update(key(KeyCode::Enter));
    assert_eq!(app.tab, Tab::Changes);
    assert!(
        app.status.as_deref().unwrap().starts_with("git diff"),
        "{:?}",
        app.status
    );

    press(&mut app, "dabc");
    app.update(key(KeyCode::Esc));
    assert!(app.prompt.is_none());
}

#[test]
fn the_changes_filter_matches_reference_and_title() {
    let repo = http_repo("filter");
    let mut app = app(&repo.root, 50);
    press(&mut app, "3/nginx");
    assert_eq!(app.filtered_changes().len(), 1);
    app.update(key(KeyCode::Backspace));
    app.update(key(KeyCode::Esc));
    assert!(!app.filtering);
    assert_eq!(app.filtered_changes().len(), 2);
    press(&mut app, "/#7");
    app.update(key(KeyCode::Enter));
    app.update(key(KeyCode::Enter));
    assert_eq!(app.blast.as_ref().unwrap().change.reference, "#7");
}

#[test]
fn outside_a_git_repository_the_changes_tab_says_why() {
    let root = std::env::temp_dir().join(format!("vernier-tui-nogit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    copy_dir(&fixture("edges-http-app"), &root);
    let _cleanup = TempRepo { root: root.clone() };
    let mut app = app(&root, 50);
    press(&mut app, "3");
    let text = render(&app, 100, 30);
    // The temp directory may itself sit inside a repository on some
    // machines; then the history is read from there and the tab lists it.
    assert!(
        text.contains("is not inside a") || text.contains("CHANGE HISTORY"),
        "{text}"
    );
}

#[test]
fn infrastructure_says_the_walk_starts_from_code() {
    let repo = repo("edges-events-app", "infra");
    let mut app = app(&repo.root, 0);
    press(&mut app, "2G");
    let service = app.selected_service().unwrap().clone();
    assert_eq!(service.role, blastradius::ServiceRole::Infrastructure);
    app.update(key(KeyCode::Enter));
    assert_eq!(app.tab, Tab::Services);
    assert_eq!(
        app.status.as_deref(),
        Some("infrastructure has no files here; the walk starts from code")
    );
    let text = render(&app, 100, 30);
    assert!(text.contains("infrastructure has no files here"), "{text}");
    // The next key clears it.
    press(&mut app, "k");
    assert!(app.status.is_none());
}

#[test]
fn quit_help_and_tabs() {
    let repo = http_repo("keys");
    let mut app = app(&repo.root, 0);
    app.update(key(KeyCode::Tab));
    assert_eq!(app.tab, Tab::Services);
    app.update(key(KeyCode::BackTab));
    app.update(key(KeyCode::BackTab));
    assert_eq!(app.tab, Tab::Blast);
    press(&mut app, "?");
    assert!(app.help);
    // Any key closes help and does nothing else.
    assert_eq!(app.update(key(KeyCode::Char('q'))), Action::Continue);
    assert!(!app.help);
    assert_eq!(
        app.update(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        Action::Quit
    );
    assert_eq!(app.update(key(KeyCode::Char('q'))), Action::Quit);
}

#[test]
fn a_change_from_the_command_line_opens_on_the_blast_tab() {
    let repo = http_repo("initial");
    let change = blastradius::git::pull_request(&repo.root, 7).unwrap();
    let app = App::new(
        analysis(&repo.root),
        Options {
            depth: 2,
            history: 0,
            color: false,
            change: Some(change),
        },
    );
    assert_eq!(app.tab, Tab::Blast);
    assert_eq!(app.depth, 2);
    let b = app.blast.as_ref().unwrap();
    assert_eq!(b.change.reference, "#7");
    assert_eq!(b.depth, 2);
}

/// Every corpus repository draws every tab at two sizes without a panic,
/// and the Blast tab's headline for the widest service is the report's.
/// Skips with a message when corpus/ is absent.
#[test]
fn corpus_draws_at_every_size() {
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
    if !corpus.is_dir() {
        eprintln!("corpus/ missing: run scripts/corpus.sh; skipping");
        return;
    }
    let mut repos: Vec<PathBuf> = std::fs::read_dir(&corpus)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    repos.sort();
    for root in repos {
        let mut app = app(&root, 0);
        let widest = blast::widest(&app.analysis.graph, app.depth);
        for tab in ["1", "2", "3", "4"] {
            press(&mut app, tab);
            render(&app, 80, 24);
            render(&app, 120, 40);
        }
        press(&mut app, "2G");
        render(&app, 80, 24);
        let Some((name, reached)) = widest else {
            continue;
        };
        let index = app
            .filtered_services()
            .iter()
            .position(|s| s.name == name)
            .unwrap();
        press(&mut app, "g");
        for _ in 0..index {
            press(&mut app, "j");
        }
        app.update(key(KeyCode::Enter));
        assert_eq!(app.tab, Tab::Blast, "{}", root.display());
        let text = render(&app, 120, 40);
        let noun = if reached == 1 { "service" } else { "services" };
        let headline = format!("-> {reached} {noun} in the blast radius");
        assert!(
            text.contains(&headline),
            "{}: widest {name} reaches {reached}\n{text}",
            root.display()
        );
        render(&app, 80, 24);
    }
}
