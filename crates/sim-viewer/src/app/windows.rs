//! Floating windows: which are open and where, dragging and scrolling them,
//! the history the charts draw, and what each window shows.

use sim_core::{AgentActivity, AgentId, Engine, Facts, TICKS_PER_YEAR};

use super::ViewerApp;
use crate::{
    feed::Tone,
    labels,
    render::{
        LineStyle, PeopleSort, Series, UiAction, WindowBody, WindowKind, WindowLine, WindowView,
    },
};

/// An open window: what it is, where its top-left corner is, and how far
/// down it's scrolled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct OpenWindow {
    pub(super) kind: WindowKind,
    pub(super) x: f32,
    pub(super) y: f32,
    pub(super) scroll: usize,
}

/// Samples taken per simulated year for the charts.
const SAMPLES_PER_YEAR: u64 = 24;
/// Samples kept; past it, neighbours are merged so the chart covers all time.
const MAX_SAMPLES: usize = 240;

/// One moment of the band's history.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Sample {
    people: u32,
    huts: u32,
    fires: u32,
    born: u32,
    died: u32,
    cold: u32,
}

/// How the band has done over time, for the charts.
#[derive(Debug, Default)]
pub(super) struct History {
    samples: Vec<Sample>,
    /// How many sampling periods each kept sample covers.
    span: u64,
}

impl History {
    /// Takes a sample when one is due.
    pub(super) fn record(&mut self, engine: &Engine, born: u32) {
        let snapshot = engine.snapshot();
        if snapshot.tick % (TICKS_PER_YEAR / SAMPLES_PER_YEAR) != 0 {
            return;
        }
        self.span = self.span.max(1);
        let period = snapshot.tick / (TICKS_PER_YEAR / SAMPLES_PER_YEAR);
        if period % self.span != 0 {
            return;
        }
        let now = (snapshot.tick / 60) as u32;
        let mut sample = Sample {
            people: snapshot.living_agent_count,
            born,
            ..Sample::default()
        };
        for structure in engine.structure_views(usize::MAX) {
            if structure.state != sim_core::StructureState::Complete {
                continue;
            }
            if structure.kind.burns() {
                sample.fires += u32::from(structure.working(now));
            } else {
                sample.huts += 1;
            }
        }
        for death in engine.death_records() {
            sample.died += 1;
            sample.cold += u32::from(death.cause == sim_core::DeathCause::Exposure);
        }
        self.samples.push(sample);
        if self.samples.len() > MAX_SAMPLES {
            // Keep every other sample: the chart now covers twice the time.
            self.samples = self.samples.iter().step_by(2).copied().collect();
            self.span *= 2;
        }
    }

    pub(super) fn clear(&mut self) {
        self.samples.clear();
        self.span = 1;
    }

    fn series(&self) -> Vec<Series> {
        let take = |pick: fn(&Sample) -> u32| self.samples.iter().map(pick).collect();
        // Deaths and births per period, from the running totals.
        let per_period = |pick: fn(&Sample) -> u32| {
            let mut previous = 0;
            self.samples
                .iter()
                .map(|sample| {
                    let value = pick(sample).saturating_sub(previous);
                    previous = pick(sample);
                    value
                })
                .collect()
        };
        let last = self.samples.last().copied().unwrap_or_default();
        vec![
            Series {
                label: "People",
                style: LineStyle::Good,
                values: take(|sample| sample.people),
                total: None,
            },
            Series {
                label: "Huts",
                style: LineStyle::Normal,
                values: take(|sample| sample.huts),
                total: None,
            },
            Series {
                label: "Fires burning",
                style: LineStyle::Talk,
                values: take(|sample| sample.fires),
                total: None,
            },
            Series {
                label: "Born",
                style: LineStyle::Heading,
                values: per_period(|sample| sample.born),
                total: Some(last.born),
            },
            Series {
                label: "Died (all causes)",
                style: LineStyle::Bad,
                values: per_period(|sample| sample.died),
                total: Some(last.died),
            },
            Series {
                label: "Died of cold",
                style: LineStyle::Dim,
                values: per_period(|sample| sample.cold),
                total: Some(last.cold),
            },
        ]
    }

    fn years(&self) -> u32 {
        (self.samples.len() as u64 * self.span.max(1) / SAMPLES_PER_YEAR) as u32
    }
}

impl ViewerApp {
    /// Opens `kind`, or brings it to the front if it's already open.
    pub(super) fn open_window(&mut self, kind: WindowKind) {
        self.windows_menu = false;
        if let Some(index) = self.windows.iter().position(|window| window.kind == kind) {
            let window = self.windows.remove(index);
            self.windows.push(window);
            return;
        }
        // Each new window a little down and right of the last, from the left.
        let step = 24.0 * self.windows.len() as f32;
        self.windows.push(OpenWindow {
            kind,
            x: 40.0 + step,
            y: 60.0 + step,
            scroll: 0,
        });
    }

    pub(super) fn close_window(&mut self, index: usize) {
        if index < self.windows.len() {
            self.windows.remove(index);
        }
        self.window_drag = None;
    }

    /// Brings the window at `index` to the front; returns its new index.
    pub(super) fn raise_window(&mut self, index: usize) -> usize {
        if index + 1 < self.windows.len() {
            let window = self.windows.remove(index);
            self.windows.push(window);
        }
        self.windows.len().saturating_sub(1)
    }

    /// Starts dragging the window at `index` by its title bar.
    pub(super) fn grab_window(&mut self, index: usize) {
        let index = self.raise_window(index);
        if let (Some(window), Some((x, y))) = (self.windows.get(index), self.cursor) {
            self.window_drag = Some((index, x as f32 - window.x, y as f32 - window.y));
        }
    }

    /// Moves a dragged window with the mouse; returns whether one is being dragged.
    pub(super) fn drag_window(&mut self, x: f64, y: f64) -> bool {
        let Some((index, dx, dy)) = self.window_drag else {
            return false;
        };
        if let Some(window) = self.windows.get_mut(index) {
            window.x = (x as f32 - dx).max(0.0);
            window.y = (y as f32 - dy).max(0.0);
        }
        true
    }

    /// Scrolls the window at `index` by `lines` (down when positive).
    pub(super) fn scroll_window(&mut self, index: usize, lines: i32) {
        if let Some(window) = self.windows.get_mut(index) {
            window.scroll = window.scroll.saturating_add_signed(lines as isize);
        }
    }

    /// What every open window shows this frame, back to front.
    pub(super) fn window_views(&mut self) -> Vec<WindowView> {
        let mut views = Vec::with_capacity(self.windows.len());
        for index in 0..self.windows.len() {
            let window = self.windows[index];
            let (title, body) = match window.kind {
                WindowKind::Charts => (
                    "Charts".to_owned(),
                    WindowBody::Charts {
                        series: self.history.series(),
                        years: self.history.years(),
                    },
                ),
                kind => {
                    let (title, lines) = match kind {
                        WindowKind::Story => ("Story".to_owned(), self.story_lines()),
                        WindowKind::Family => ("Family".to_owned(), self.family_lines()),
                        WindowKind::People => ("People".to_owned(), self.people_lines()),
                        _ => self.mind_lines(),
                    };
                    let (chars, rows) = kind.size();
                    let lines = wrapped(lines, chars);
                    let total = lines.len();
                    let first = window.scroll.min(total.saturating_sub(rows));
                    self.windows[index].scroll = first;
                    (
                        title,
                        WindowBody::Lines {
                            lines: lines.into_iter().skip(first).take(rows).collect(),
                            first,
                            total,
                        },
                    )
                }
            };
            views.push(WindowView {
                kind: window.kind,
                title,
                x: window.x,
                y: window.y,
                body,
            });
        }
        views
    }

    /// The band's story, newest year first.
    fn story_lines(&self) -> Vec<WindowLine> {
        let mut lines = Vec::new();
        let mut year = None;
        for told in self.feed.story().rev() {
            if year != Some(told.year) {
                year = Some(told.year);
                lines.push(WindowLine::new(
                    format!("Year {}", told.year),
                    LineStyle::Heading,
                ));
            }
            let style = match told.entry.tone {
                Tone::Bad => LineStyle::Bad,
                Tone::Good => LineStyle::Good,
                Tone::Talk => LineStyle::Talk,
                Tone::Neutral => LineStyle::Dim,
            };
            lines.push(
                WindowLine::new(told.entry.text.clone(), style)
                    .indented(1)
                    .on_click(UiAction::LookAt(told.entry.position, told.entry.agent)),
            );
        }
        if lines.is_empty() {
            lines.push(WindowLine::new("Nothing has happened yet", LineStyle::Dim));
        }
        lines
    }

    /// Everyone who has lived, under their parent, with their partner.
    fn family_lines(&self) -> Vec<WindowLine> {
        let everyone: Vec<_> = self.engine.agent_views(usize::MAX).collect();
        let parent: Vec<Option<AgentId>> = everyone
            .iter()
            .map(|person| self.engine.parent_of(person.id))
            .collect();
        let mut lines = Vec::new();
        let mut stack: Vec<(usize, u8)> = everyone
            .iter()
            .enumerate()
            .rev()
            .filter(|(index, _)| parent[*index].is_none())
            .map(|(index, _)| (index, 0))
            .collect();
        let mut listed: Vec<AgentId> = Vec::new();
        while let Some((index, depth)) = stack.pop() {
            let person = everyone[index];
            // A couple shows once, under whichever of them comes first.
            if listed.contains(&person.id) {
                continue;
            }
            listed.push(person.id);
            let dead = person.activity == AgentActivity::Dead;
            let mut text = self.name(person.id);
            if let Some(life) = self.engine.life(person.id) {
                text.push_str(&format!(", {}", life.age));
            }
            if let Some(partner) = self.engine.partner_of(person.id) {
                text.push_str(&format!(" · with {}", self.name(partner)));
                if depth == 0
                    && parent
                        .get(partner.get() as usize)
                        .copied()
                        .flatten()
                        .is_none()
                {
                    listed.push(partner);
                }
            }
            if dead {
                text.push_str(" (died)");
            }
            lines.push(
                WindowLine::new(
                    text,
                    if dead {
                        LineStyle::Dim
                    } else {
                        LineStyle::Normal
                    },
                )
                .indented(depth.min(6))
                .on_click(UiAction::Pick(person.id)),
            );
            for child in (0..everyone.len()).rev() {
                if parent[child] == Some(person.id) {
                    stack.push((child, depth + 1));
                }
            }
        }
        if lines.is_empty() {
            lines.push(WindowLine::new("Nobody yet", LineStyle::Dim));
        }
        lines
    }

    /// Everyone alive, sorted as chosen; the header changes the order.
    fn people_lines(&self) -> Vec<WindowLine> {
        let mut people: Vec<(String, u32, String, AgentId)> = self
            .engine
            .agent_views(usize::MAX)
            .filter(|person| person.activity != AgentActivity::Dead)
            .map(|person| {
                let age = self.engine.life(person.id).map_or(0, |life| life.age);
                let doing = if person.activity == AgentActivity::Sleeping {
                    "Sleeping".to_owned()
                } else {
                    self.engine
                        .physical_policy(person.id)
                        .map_or_else(String::new, |policy| labels::goal(policy.goal).to_owned())
                };
                (self.name(person.id), age, doing, person.id)
            })
            .collect();
        match self.people_sort {
            PeopleSort::Name => people.sort_by(|a, b| a.0.cmp(&b.0)),
            PeopleSort::Age => people.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0))),
            PeopleSort::Doing => people.sort_by(|a, b| a.2.cmp(&b.2).then(a.0.cmp(&b.0))),
        }
        let next = match self.people_sort {
            PeopleSort::Name => PeopleSort::Age,
            PeopleSort::Age => PeopleSort::Doing,
            PeopleSort::Doing => PeopleSort::Name,
        };
        let by = match self.people_sort {
            PeopleSort::Name => "name",
            PeopleSort::Age => "age",
            PeopleSort::Doing => "what they're doing",
        };
        let mut lines = vec![
            WindowLine::new(
                format!("{} people, by {by} (click to change)", people.len()),
                LineStyle::Heading,
            )
            .on_click(UiAction::SortPeople(next)),
        ];
        for (name, age, doing, id) in people {
            lines.push(
                WindowLine::new(format!("{name:<12}{age:>3}  {doing}"), LineStyle::Normal)
                    .on_click(UiAction::Pick(id)),
            );
        }
        lines
    }

    /// The picked person's plan, how they think the world works, and whom they trust.
    fn mind_lines(&self) -> (String, Vec<WindowLine>) {
        let Some(agent) = self.selected else {
            return (
                "Mind".to_owned(),
                vec![WindowLine::new(
                    "Pick someone to see what they think",
                    LineStyle::Dim,
                )],
            );
        };
        let mut lines = Vec::new();
        let plan = self
            .engine
            .intention(agent)
            .and_then(labels::plan)
            .unwrap_or_else(|| "No plan right now".to_owned());
        lines.push(WindowLine::new("Thinking", LineStyle::Heading));
        lines.push(WindowLine::new(plan, LineStyle::Normal).indented(1));
        let Some(mind) = self.engine.mental_map(agent) else {
            return (format!("Mind of {}", self.name(agent)), lines);
        };
        lines.push(WindowLine::new(
            "How they think things work",
            LineStyle::Heading,
        ));
        let mut beliefs = 0;
        for belief in &mind.beliefs {
            let Some(sentence) = belief_sentence(belief) else {
                continue;
            };
            beliefs += 1;
            lines.push(WindowLine::new(sentence, LineStyle::Normal).indented(1));
        }
        if beliefs == 0 {
            lines.push(WindowLine::new("Nothing learned yet", LineStyle::Dim).indented(1));
        }
        lines.push(WindowLine::new("Places they know", LineStyle::Heading));
        let mut kinds: Vec<(&'static str, usize)> = Vec::new();
        for place in &mind.landmarks {
            let label = labels::concept(place.kind.concept());
            match kinds.iter_mut().find(|(known, _)| *known == label) {
                Some((_, count)) => *count += 1,
                None => kinds.push((label, 1)),
            }
        }
        let known = kinds
            .iter()
            .map(|(label, count)| format!("{label} {count}"))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(
            WindowLine::new(
                if known.is_empty() {
                    "None yet".to_owned()
                } else {
                    known
                },
                LineStyle::Normal,
            )
            .indented(1),
        );
        lines.push(WindowLine::new("Whom they trust most", LineStyle::Heading));
        let mut people = mind.acquaintances.clone();
        people.sort_by(|a, b| b.trust.cmp(&a.trust).then(a.agent.cmp(&b.agent)));
        for person in people.iter().take(5) {
            let name = person
                .name
                .map_or_else(|| labels::person(person.agent), |name| name.spoken());
            lines.push(
                WindowLine::new(
                    format!("{name} (trust {})", person.trust),
                    LineStyle::Normal,
                )
                .indented(1)
                .on_click(UiAction::Pick(person.agent)),
            );
        }
        (format!("Mind of {}", self.name(agent)), lines)
    }

    fn name(&self, agent: AgentId) -> String {
        self.engine
            .life(agent)
            .map_or_else(|| labels::person(agent), |life| life.name.spoken())
    }
}

/// `lines` wrapped to `chars` across, continuations indented one more.
fn wrapped(lines: Vec<WindowLine>, chars: usize) -> Vec<WindowLine> {
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        let room = chars
            .saturating_sub(2 * usize::from(line.indent) + 1)
            .max(8);
        for (index, text) in crate::render::wrap(&line.text, room)
            .into_iter()
            .enumerate()
        {
            let mut piece = WindowLine {
                text,
                ..line.clone()
            };
            if index > 0 {
                piece.indent += 1;
            }
            out.push(piece);
        }
    }
    out
}

/// One belief in words: "Feeding a fire: needs carrying wood; brings a fire
/// burning (9 in 10); eases cold". None when there's nothing to say.
fn belief_sentence(belief: &sim_core::BeliefView) -> Option<String> {
    let mut parts = Vec::new();
    let needs: Vec<&str> = (0..16)
        .map(|bit| 1_u16 << bit)
        .filter(|&fact| belief.needs.has(fact))
        .map(Facts::name)
        .collect();
    if !needs.is_empty() {
        parts.push(format!("needs {}", needs.join(" and ")));
    }
    let makes: Vec<String> = belief
        .produces
        .iter()
        .filter(|&&(_, hits, tries)| tries >= 3 && hits * 2 >= tries)
        .map(|&(fact, hits, tries)| format!("{} ({hits} in {tries})", Facts::name(fact)))
        .collect();
    if !makes.is_empty() {
        parts.push(format!("brings {}", makes.join(", ")));
    }
    let eases: Vec<&str> = ["hunger", "thirst", "tiredness", "cold"]
        .into_iter()
        .zip(belief.relieves)
        .filter(|&(_, relief)| relief >= 5)
        .map(|(need, _)| need)
        .collect();
    if !eases.is_empty() {
        parts.push(format!("eases {}", eases.join(" and ")));
    }
    (!parts.is_empty()).then(|| format!("{}: {}", labels::action(belief.choice), parts.join("; ")))
}
