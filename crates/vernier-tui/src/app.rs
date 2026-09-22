//! The TUI's state and how keys change it. Nothing here touches the
//! terminal; the only I/O is git, for a diff range typed at the prompt.
use std::cell::Cell;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use vernier::history::{self, History};
use vernier::{Analysis, Blast, Change, Edge, Service, ServiceRole, blast, git};

use crate::theme::Theme;
use crate::tree;

/// The deepest walk `+` allows. Past this every repository in the corpus has
/// long stopped growing.
pub const MAX_DEPTH: usize = 20;

/// Lines `PgUp` and `PgDn` move.
const PAGE: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Services,
    Changes,
    Blast,
}

impl Tab {
    pub const ALL: [Self; 4] = [Self::Overview, Self::Services, Self::Changes, Self::Blast];

    pub fn title(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Services => "Services",
            Self::Changes => "Changes",
            Self::Blast => "Blast",
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }
}

/// Which side of the Services tab has the keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    List,
    Edges,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    Diff,
    Files,
}

impl PromptKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Diff => "diff range",
            Self::Files => "files",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub kind: PromptKind,
    pub text: String,
}

/// Which way an edge points from the selected service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// The selected service depends on the other end.
    DependsOn,
    /// The other end depends on the selected service.
    DependedOnBy,
}

/// The Changes tab: the history's numbers and, in the same order as its
/// entries, the changes they were computed from.
#[derive(Debug, Clone)]
pub struct Recent {
    pub history: History,
    pub changes: Vec<Change>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Continue,
    Quit,
}

/// What `run` is asked to show.
#[derive(Debug, Clone)]
pub struct Options {
    pub depth: usize,
    /// How many recent pull requests the Changes tab lists. 0 lists none.
    pub history: usize,
    pub color: bool,
    /// Open straight on this change.
    pub change: Option<Change>,
}

pub struct App {
    pub analysis: Analysis,
    /// Code first, then by name, as the reports order them.
    pub services: Vec<Service>,
    /// The repository report, one entry per line.
    pub overview: Vec<String>,
    pub recent: Result<Recent, String>,
    pub theme: Theme,
    pub depth: usize,
    /// Every code service and how many services a change to it reaches at
    /// `depth`, widest first; ties keep the services' order, as the
    /// report's widest-change-surface finding breaks them.
    pub surface: Vec<(String, usize)>,
    pub tab: Tab,
    pub scroll: usize,
    pub pane: Pane,
    pub service_filter: String,
    pub service_sel: usize,
    pub edge_sel: usize,
    pub change_filter: String,
    pub change_sel: usize,
    /// `/` was pressed and keys go to the focused list's filter.
    pub filtering: bool,
    pub blast: Option<Blast>,
    /// The walk drawn as a tree; `reached_sel` indexes it.
    pub tree: Vec<tree::Row>,
    pub reached_sel: usize,
    /// Where `Esc` on the Blast tab goes.
    pub opened_from: Tab,
    pub prompt: Option<Prompt>,
    /// One line for the footer until the next key: an error, or why a key
    /// did nothing.
    pub status: Option<String>,
    pub help: bool,
    /// First visible row of each list. The renderer keeps the selection in
    /// view and writes the offset back, so scrolling does not jump.
    pub offsets: Offsets,
}

#[derive(Debug, Default)]
pub struct Offsets {
    pub services: Cell<usize>,
    pub edges: Cell<usize>,
    pub changes: Cell<usize>,
    pub reached: Cell<usize>,
}

impl App {
    /// Reads the history (one git call per entry) and, when asked, walks the
    /// first change. Call before the terminal is taken over.
    pub fn new(analysis: Analysis, options: Options) -> Self {
        let services = analysis.graph.services();
        let overview = vernier::format_repo_report(&analysis, false)
            .lines()
            .map(str::to_string)
            .collect();
        let recent = if options.history == 0 {
            Err("--history 0: no changes listed".to_string())
        } else {
            load_recent(&analysis, options.history, options.depth)
        };
        let mut app = Self {
            analysis,
            services,
            overview,
            recent,
            theme: Theme::new(options.color),
            depth: options.depth.clamp(1, MAX_DEPTH),
            surface: Vec::new(),
            tab: Tab::Overview,
            scroll: 0,
            pane: Pane::List,
            service_filter: String::new(),
            service_sel: 0,
            edge_sel: 0,
            change_filter: String::new(),
            change_sel: 0,
            filtering: false,
            blast: None,
            tree: Vec::new(),
            reached_sel: 0,
            opened_from: Tab::Overview,
            prompt: None,
            status: None,
            help: false,
            offsets: Offsets::default(),
        };
        app.surface = surface(&app.analysis, app.depth);
        if let Some(change) = options.change {
            app.open_blast(change, Tab::Changes);
        }
        app
    }

    /// The services the filter keeps, in list order.
    pub fn filtered_services(&self) -> Vec<&Service> {
        let needle = self.service_filter.to_lowercase();
        self.services
            .iter()
            .filter(|s| {
                needle.is_empty()
                    || s.name.to_lowercase().contains(&needle)
                    || s.language
                        .as_deref()
                        .is_some_and(|l| l.to_lowercase().contains(&needle))
            })
            .collect()
    }

    pub fn selected_service(&self) -> Option<&Service> {
        self.filtered_services().get(self.service_sel).copied()
    }

    /// The selected service's edges: what it depends on, then what depends
    /// on it, each in the graph's order.
    pub fn edges_of_selected(&self) -> Vec<(Direction, &Edge)> {
        let Some(service) = self.selected_service() else {
            return Vec::new();
        };
        let graph = &self.analysis.graph;
        graph
            .outbound(&service.name)
            .into_iter()
            .map(|e| (Direction::DependsOn, e))
            .chain(
                graph
                    .inbound(&service.name)
                    .into_iter()
                    .map(|e| (Direction::DependedOnBy, e)),
            )
            .collect()
    }

    pub fn selected_edge(&self) -> Option<(Direction, &Edge)> {
        self.edges_of_selected().get(self.edge_sel).copied()
    }

    /// Indices into the history's entries that the filter keeps.
    pub fn filtered_changes(&self) -> Vec<usize> {
        let Ok(recent) = &self.recent else {
            return Vec::new();
        };
        let needle = self.change_filter.to_lowercase();
        recent
            .history
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                needle.is_empty()
                    || e.reference.to_lowercase().contains(&needle)
                    || e.title.to_lowercase().contains(&needle)
            })
            .map(|(i, _)| i)
            .collect()
    }

    pub fn update(&mut self, key: KeyEvent) -> Action {
        if key.kind == KeyEventKind::Release {
            return Action::Continue;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Action::Quit;
        }
        self.status = None;
        if self.help {
            self.help = false;
            return Action::Continue;
        }
        if self.prompt.is_some() {
            self.prompt_key(key);
            return Action::Continue;
        }
        if self.filtering {
            self.filter_key(key);
            return Action::Continue;
        }
        match key.code {
            KeyCode::Char('q') => return Action::Quit,
            KeyCode::Char('?') => self.help = true,
            KeyCode::Char(c @ '1'..='4') => {
                self.tab = Tab::ALL[(c as usize) - ('1' as usize)];
            }
            KeyCode::Tab => self.tab = Tab::ALL[(self.tab.index() + 1) % Tab::ALL.len()],
            KeyCode::BackTab => {
                self.tab = Tab::ALL[(self.tab.index() + Tab::ALL.len() - 1) % Tab::ALL.len()];
            }
            _ => match self.tab {
                Tab::Overview => self.overview_key(key),
                Tab::Services => self.services_key(key),
                Tab::Changes => self.changes_key(key),
                Tab::Blast => self.blast_key(key),
            },
        }
        Action::Continue
    }

    fn overview_key(&mut self, key: KeyEvent) {
        let last = self.overview.len().saturating_sub(1);
        self.scroll = moved(self.scroll, last, key.code).unwrap_or(self.scroll);
    }

    fn services_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('h') | KeyCode::Left => self.pane = Pane::List,
            KeyCode::Char('l') | KeyCode::Right => {
                if !self.edges_of_selected().is_empty() {
                    self.pane = Pane::Edges;
                }
            }
            KeyCode::Char('/') => {
                self.pane = Pane::List;
                self.filtering = true;
            }
            KeyCode::Esc => {
                if self.pane == Pane::Edges {
                    self.pane = Pane::List;
                } else {
                    self.set_service_filter(String::new());
                }
            }
            KeyCode::Enter => self.blast_selected_service(),
            code => match self.pane {
                Pane::List => {
                    let last = self.filtered_services().len().saturating_sub(1);
                    if let Some(sel) = moved(self.service_sel, last, code) {
                        if sel != self.service_sel {
                            self.service_sel = sel;
                            self.edge_sel = 0;
                            self.offsets.edges.set(0);
                        }
                    }
                }
                Pane::Edges => {
                    let last = self.edges_of_selected().len().saturating_sub(1);
                    self.edge_sel = moved(self.edge_sel, last, code).unwrap_or(self.edge_sel);
                }
            },
        }
    }

    fn changes_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('/') => self.filtering = true,
            KeyCode::Esc => self.set_change_filter(String::new()),
            KeyCode::Char('d') => {
                self.prompt = Some(Prompt {
                    kind: PromptKind::Diff,
                    text: String::new(),
                });
            }
            KeyCode::Char('f') => {
                self.prompt = Some(Prompt {
                    kind: PromptKind::Files,
                    text: String::new(),
                });
            }
            KeyCode::Enter => {
                let index = self.filtered_changes().get(self.change_sel).copied();
                let change = match (&self.recent, index) {
                    (Ok(recent), Some(i)) => recent.changes.get(i).cloned(),
                    _ => None,
                };
                if let Some(change) = change {
                    self.open_blast(change, Tab::Changes);
                }
            }
            code => {
                let last = self.filtered_changes().len().saturating_sub(1);
                self.change_sel = moved(self.change_sel, last, code).unwrap_or(self.change_sel);
            }
        }
    }

    fn blast_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('+' | '=') => self.set_depth(self.depth + 1),
            KeyCode::Char('-' | '_') => self.set_depth(self.depth.saturating_sub(1)),
            KeyCode::Esc => {
                if self.opened_from != Tab::Blast {
                    self.tab = self.opened_from;
                }
            }
            code => {
                let last = self.tree.len().saturating_sub(1);
                self.reached_sel = moved(self.reached_sel, last, code).unwrap_or(self.reached_sel);
            }
        }
    }

    fn filter_key(&mut self, key: KeyEvent) {
        let mut text = match self.tab {
            Tab::Changes => self.change_filter.clone(),
            _ => self.service_filter.clone(),
        };
        match key.code {
            KeyCode::Enter => {
                self.filtering = false;
                return;
            }
            KeyCode::Esc => {
                self.filtering = false;
                text.clear();
            }
            KeyCode::Backspace => {
                text.pop();
            }
            KeyCode::Char(c) => text.push(c),
            _ => return,
        }
        match self.tab {
            Tab::Changes => self.set_change_filter(text),
            _ => self.set_service_filter(text),
        }
    }

    fn prompt_key(&mut self, key: KeyEvent) {
        let Some(prompt) = self.prompt.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.prompt = None,
            KeyCode::Backspace => {
                prompt.text.pop();
            }
            KeyCode::Char(c) => prompt.text.push(c),
            KeyCode::Enter => {
                let Prompt { kind, text } = self.prompt.take().expect("checked above");
                let text = text.trim().to_string();
                if text.is_empty() {
                    return;
                }
                match kind {
                    PromptKind::Diff => match git::diff(&self.analysis.root, &text) {
                        Ok(change) => self.open_blast(change, Tab::Changes),
                        Err(e) => self.status = Some(e.to_string()),
                    },
                    PromptKind::Files => {
                        let files: Vec<String> =
                            text.split_whitespace().map(str::to_string).collect();
                        self.open_blast(Change::from_files(&files), Tab::Changes);
                    }
                }
            }
            _ => {}
        }
    }

    fn set_service_filter(&mut self, text: String) {
        self.service_filter = text;
        self.service_sel = 0;
        self.edge_sel = 0;
        self.offsets.services.set(0);
        self.offsets.edges.set(0);
    }

    fn set_change_filter(&mut self, text: String) {
        self.change_filter = text;
        self.change_sel = 0;
        self.offsets.changes.set(0);
    }

    /// A change to the selected service: its root as the changed file, so
    /// ownership is decided the way it is for any real file there.
    fn blast_selected_service(&mut self) {
        let Some(service) = self.selected_service() else {
            return;
        };
        let (Some(root), ServiceRole::Code) = (service.root.clone(), service.role) else {
            self.status =
                Some("infrastructure has no files here; the walk starts from code".into());
            return;
        };
        let mut change = Change::from_files(&[root]);
        change.reference = format!("service {}", service.name);
        self.open_blast(change, Tab::Services);
    }

    pub fn open_blast(&mut self, change: Change, from: Tab) {
        self.set_blast(blast::of_change(&self.analysis.graph, change, self.depth));
        // The first reached service, when there is one, else the root.
        self.reached_sel = self
            .tree
            .iter()
            .position(|r| matches!(r.kind, tree::Kind::Reached(_)))
            .unwrap_or(0);
        self.offsets.reached.set(0);
        self.opened_from = from;
        self.tab = Tab::Blast;
    }

    fn set_depth(&mut self, depth: usize) {
        let depth = depth.clamp(1, MAX_DEPTH);
        if depth == self.depth {
            self.status = Some(format!("depth stays between 1 and {MAX_DEPTH}"));
            return;
        }
        self.depth = depth;
        self.surface = surface(&self.analysis, depth);
        if let Some(b) = self.blast.take() {
            let selected = self.tree.get(self.reached_sel).map(|r| r.service.clone());
            self.set_blast(blast::of_change(&self.analysis.graph, b.change, depth));
            // Keep the same service selected when it is still in the tree.
            self.reached_sel = selected
                .and_then(|name| self.tree.iter().position(|r| r.service == name))
                .unwrap_or(0);
        }
    }

    fn set_blast(&mut self, b: Blast) {
        self.tree = tree::rows(&b);
        self.blast = Some(b);
    }

    /// The tree row under the cursor on the Blast tab.
    pub fn selected_row(&self) -> Option<&tree::Row> {
        self.tree.get(self.reached_sel)
    }
}

/// The new position after a movement key, or None when the key does not
/// move.
fn moved(current: usize, last: usize, code: KeyCode) -> Option<usize> {
    let next = match code {
        KeyCode::Char('j') | KeyCode::Down => current + 1,
        KeyCode::Char('k') | KeyCode::Up => current.saturating_sub(1),
        KeyCode::PageDown => current + PAGE,
        KeyCode::PageUp => current.saturating_sub(PAGE),
        KeyCode::Char('g') | KeyCode::Home => 0,
        KeyCode::Char('G') | KeyCode::End => last,
        _ => return None,
    };
    Some(next.min(last))
}

fn surface(analysis: &Analysis, depth: usize) -> Vec<(String, usize)> {
    let graph = &analysis.graph;
    let mut out: Vec<(String, usize)> = graph
        .services()
        .into_iter()
        .filter(|s| s.role == ServiceRole::Code)
        .map(|s| {
            let n = blast::radius(graph, std::slice::from_ref(&s.name), depth)
                .reached
                .len();
            (s.name, n)
        })
        .collect();
    out.sort_by_key(|s| std::cmp::Reverse(s.1));
    out
}

fn load_recent(analysis: &Analysis, n: usize, depth: usize) -> Result<Recent, String> {
    let (unit, list) = history::changes(analysis, n).map_err(|e| e.to_string())?;
    let entries = list
        .iter()
        .map(|(commit, change)| {
            history::entry(
                commit,
                &blast::of_change(&analysis.graph, change.clone(), depth),
            )
        })
        .collect();
    Ok(Recent {
        history: history::summarise(n, unit, depth, entries),
        changes: list.into_iter().map(|(_, change)| change).collect(),
    })
}
