//! Screen-space interface: top bar, hover tooltip, person panel, event feed,
//! build palette, speech bubbles, help, and the clickable regions they leave.

use sim_core::{
    AgentActivity, Concept, HEALTH_INCAPACITATION_THRESHOLD, HEALTH_MAX, Material, NEED_MAX,
    NeedLevelView, StructureState,
};

use super::{
    AgentInspection, BuildTool, Hover, RenderState, UiAction,
    colors::{self, rgba},
    gpu::Instance,
    text::{ADVANCE, LINE, push_text, text_width, wrap},
};
use crate::{camera::CameraView, feed::Tone, gestures::GestureMark, labels};

/// A clickable or click-blocking screen rectangle from the latest frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// `None` for panels that only block clicks from reaching the map.
    pub action: Option<UiAction>,
}

impl Hit {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }
}

/// Characters across the person panel's text column.
const PANEL_CHARS: usize = 32;
/// Characters across the event feed.
const FEED_CHARS: usize = 40;
/// Characters before a panel value column starts.
const LABEL_CHARS: usize = 10;
/// Characters before a grid cell's value starts.
const GRID_LABEL_CHARS: usize = 9;

struct Painter<'a> {
    out: &'a mut Vec<Instance>,
    hits: &'a mut Vec<Hit>,
    /// Font pixel size; every other measure is a multiple of it.
    px: f32,
    cursor: Option<(f32, f32)>,
}

impl Painter<'_> {
    fn line(&self) -> f32 {
        LINE * self.px
    }

    fn pad(&self) -> f32 {
        4.0 * self.px
    }

    fn chars(&self, count: usize) -> f32 {
        count as f32 * ADVANCE * self.px
    }

    fn rect(&mut self, x: f32, y: f32, width: f32, height: f32, color: u32) {
        self.out.push(Instance::new(x, y, width, height, color));
    }

    fn panel(&mut self, x: f32, y: f32, width: f32, height: f32) {
        self.rect(x, y, width, height, colors::UI_PANEL);
        self.outline(x, y, width, height, colors::UI_BORDER);
        self.hits.push(Hit {
            x,
            y,
            width,
            height,
            action: None,
        });
    }

    fn outline(&mut self, x: f32, y: f32, width: f32, height: f32, color: u32) {
        let line = (self.px / 2.0).max(1.0);
        self.rect(x, y, width, line, color);
        self.rect(x, y + height - line, width, line, color);
        self.rect(x, y, line, height, color);
        self.rect(x + width - line, y, line, height, color);
    }

    /// Text in a line box whose top is `y`; returns the x just past it.
    fn text(&mut self, text: &str, x: f32, y: f32, color: u32) -> f32 {
        push_text(self.out, text, x, y + self.px, self.px, color)
    }

    fn hovered(&self, x: f32, y: f32, width: f32, height: f32) -> bool {
        self.cursor
            .is_some_and(|(cx, cy)| cx >= x && cy >= y && cx < x + width && cy < y + height)
    }

    fn button_width(&self, label: &str) -> f32 {
        text_width(label, self.px) + 2.0 * 3.0 * self.px
    }

    /// A button with its top-left at (`x`, `y`); returns its width.
    fn button(&mut self, label: &str, x: f32, y: f32, action: UiAction, active: bool) -> f32 {
        let width = self.button_width(label);
        let height = self.line() + 2.0 * self.px;
        let color = if active {
            colors::UI_BUTTON_ACTIVE
        } else if self.hovered(x, y, width, height) {
            colors::UI_BUTTON_HOVER
        } else {
            colors::UI_BUTTON
        };
        self.rect(x, y, width, height, color);
        self.text(label, x + 3.0 * self.px, y + self.px, colors::UI_TEXT);
        self.hits.push(Hit {
            x,
            y,
            width,
            height,
            action: Some(action),
        });
        width
    }
}

pub(super) fn build_interface(
    out: &mut Vec<Instance>,
    hits: &mut Vec<Hit>,
    state: &RenderState,
    view: CameraView,
    gestures: &[GestureMark],
    details: Option<&str>,
) {
    let (width, height) = view.screen_size();
    out.clear();
    hits.clear();
    let px = (2.0 * state.ui_scale).floor().max(2.0);
    let mut painter = Painter {
        out,
        hits,
        px,
        cursor: state.cursor.map(|(x, y)| (x as f32, y as f32)),
    };
    let painter = &mut painter;
    speech_bubbles(painter, view, gestures);
    let bar = top_bar(painter, state, width);
    let palette_top = state
        .build
        .map_or(height, |tool| build_palette(painter, tool, width, height));
    feed(painter, state, palette_top);
    if let Some(agent) = &state.selected {
        person_panel(painter, agent, state.following, width, height, bar);
    }
    if let Some(details) = details {
        details_panel(painter, details, bar);
    }
    if state.build.is_none() {
        if let Some(hint) = start_hint(state) {
            let y = height - painter.pad() - painter.line() * 2.0;
            banner(painter, hint, width, y, colors::UI_PANEL);
        }
    }
    if let Some(toast) = &state.toast {
        banner(painter, toast, width, bar + painter.pad(), colors::UI_TOAST);
    }
    if let Some(hover) = &state.hover
        && let Some((x, y)) = painter.cursor
    {
        tooltip(painter, hover, x, y, width, height);
    }
    if state.help_open {
        help(painter, width, height);
    }
}

/// Bottom-of-screen guidance before anyone lives in the world.
fn start_hint(state: &RenderState) -> Option<&'static str> {
    use super::PopulationStatus;
    match state.population_status {
        PopulationStatus::Waiting => Some("Loading the world…"),
        PopulationStatus::Ready => Some("Press T with the mouse over land to add the first person"),
        PopulationStatus::Active => None,
    }
}

/// A one-line centered message box with its top at `y`.
fn banner(painter: &mut Painter, text: &str, width: f32, y: f32, color: u32) {
    let w = text_width(text, painter.px) + 2.0 * painter.pad();
    let h = painter.line() + painter.pad();
    let x = ((width - w) / 2.0).max(0.0);
    painter.rect(x, y, w, h, color);
    painter.outline(x, y, w, h, colors::UI_BORDER);
    painter.text(
        text,
        x + painter.pad(),
        y + painter.pad() / 2.0,
        colors::UI_TEXT,
    );
}

/// Speed steps the bar's buttons move between.
pub const SPEEDS: [f32; 9] = [1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0, 256.0];

/// Draws the top bar and returns its height.
fn top_bar(painter: &mut Painter, state: &RenderState, width: f32) -> f32 {
    let px = painter.px;
    let height = painter.line() + 6.0 * px;
    painter.rect(0.0, 0.0, width, height, colors::UI_BAR);
    painter.rect(0.0, height - px / 2.0, width, px / 2.0, colors::UI_BORDER);
    painter.hits.push(Hit {
        x: 0.0,
        y: 0.0,
        width,
        height,
        action: None,
    });
    let button_y = 2.0 * px;
    let text_y = 3.0 * px;
    let gap = 2.0 * px;
    let paused = state.snapshot.paused;
    let mut x = painter.pad();
    x += painter.button(
        if paused { "▶" } else { "‖" },
        x,
        button_y,
        UiAction::TogglePause,
        paused,
    );
    x += gap * 2.0;
    x += painter.button("-", x, button_y, UiAction::Slower, false) + gap;
    let speed = format!("{}×", state.snapshot.speed);
    let speed_x = x + (painter.chars(4) - text_width(&speed, px)) / 2.0;
    painter.text(&speed, speed_x, text_y, colors::UI_TEXT);
    x += painter.chars(4) + gap;
    x += painter.button("+", x, button_y, UiAction::Faster, false);
    x += painter.chars(2);
    x = painter.text(
        &labels::duration(state.snapshot.simulated_seconds),
        x,
        text_y,
        colors::UI_TEXT,
    );
    if paused {
        x = painter.text("  Paused", x, text_y, colors::UI_WARN);
    }
    if let Some(status) = generation_note(state.generation_status) {
        x = painter.text(&format!("  {status}"), x, text_y, colors::UI_WARN);
    }

    let help_width = painter.button_width("? Help");
    let help_x = width - painter.pad() - help_width;
    painter.button("? Help", help_x, button_y, UiAction::Help, state.help_open);
    let census = &state.census;
    let mut items: Vec<(Option<u32>, String, u32)> = vec![(
        Some(colors::agent_color(AgentActivity::Moving)),
        format!("{} people", census.people),
        colors::UI_TEXT,
    )];
    if census.dead > 0 {
        items.push((None, format!("{} dead", census.dead), colors::UI_BAD));
    }
    items.push((
        Some(colors::DEER),
        format!("{} deer", census.deer),
        colors::UI_TEXT,
    ));
    items.push((
        Some(colors::WOLF),
        format!("{} wolves", census.wolves),
        colors::UI_TEXT,
    ));
    let item_width = |painter: &Painter, (icon, label, _): &(Option<u32>, String, u32)| {
        text_width(label, painter.px)
            + if icon.is_some() {
                painter.chars(2)
            } else {
                0.0
            }
            + painter.chars(2)
    };
    // Drop the least important counts when the bar is too narrow.
    while !items.is_empty()
        && x + items
            .iter()
            .map(|item| item_width(painter, item))
            .sum::<f32>()
            > help_x
    {
        items.pop();
    }
    let mut right = help_x - painter.chars(1);
    for item in items.iter().rev() {
        right -= item_width(painter, item) - painter.chars(1);
        let mut item_x = right;
        if let Some(color) = item.0 {
            let size = 5.0 * px;
            painter.rect(item_x, text_y + 2.0 * px, size, size, color);
            item_x += painter.chars(2);
        }
        painter.text(&item.1, item_x, text_y, item.2);
        right -= painter.chars(1);
    }
    height
}

const fn generation_note(status: super::GenerationStatus) -> Option<&'static str> {
    use super::GenerationStatus;
    match status {
        GenerationStatus::Idle => None,
        GenerationStatus::Bootstrap => Some("Loading world…"),
        GenerationStatus::Manual => Some("Generating selection…"),
        GenerationStatus::Cancelling => Some("Cancelling…"),
        GenerationStatus::WorkerUnavailable => Some("World generator offline"),
    }
}

/// The tooltip's lines for what is under the cursor.
pub(super) fn hover_lines(hover: &Hover) -> (String, Option<String>) {
    match *hover {
        Hover::Person { id, activity } => (
            labels::person(id),
            Some(format!(
                "{} · click to follow their story",
                labels::activity(activity)
            )),
        ),
        Hover::Animal { species, mode } => (
            capitalized(labels::species(species)),
            labels::animal_mode(mode).map(str::to_owned),
        ),
        Hover::Carcass { meat } => ("Carcass".to_owned(), Some(format!("{meat} meat left"))),
        Hover::Structure { kind, state } => (
            labels::structure(kind).to_owned(),
            (state == StructureState::UnderConstruction).then(|| "being built".to_owned()),
        ),
        Hover::Resource { label, remaining } => (
            label.to_owned(),
            remaining.map(|(amount, material)| match amount {
                0 => "picked clean".to_owned(),
                _ => format!("{amount} {} left", labels::material(material)),
            }),
        ),
        Hover::Terrain(label) => (label.to_owned(), None),
    }
}

fn tooltip(painter: &mut Painter, hover: &Hover, x: f32, y: f32, width: f32, height: f32) {
    let (title, detail) = hover_lines(hover);
    let px = painter.px;
    let text_w = text_width(&title, px).max(detail.as_deref().map_or(0.0, |d| text_width(d, px)));
    let lines = 1.0 + f32::from(u8::from(detail.is_some()));
    let w = text_w + 2.0 * painter.pad();
    let h = lines * painter.line() + painter.pad();
    let tx = (x + 8.0 * px).min(width - w - px).max(0.0);
    let ty = if y + 8.0 * px + h > height {
        y - h - 4.0 * px
    } else {
        y + 8.0 * px
    };
    painter.rect(tx, ty, w, h, colors::UI_TOOLTIP);
    painter.outline(tx, ty, w, h, colors::UI_BORDER);
    let text_x = tx + painter.pad();
    let mut line_y = ty + painter.pad() / 2.0;
    painter.text(&title, text_x, line_y, colors::UI_TEXT);
    if let Some(detail) = detail {
        line_y += painter.line();
        painter.text(&detail, text_x, line_y, colors::UI_DIM);
    }
}

/// One row of the person panel.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Row {
    Title(String),
    Text(String, u32),
    /// A dim label in a fixed column and a value beside it.
    Pair(&'static str, String),
    /// A dim label and a bar filled to `fill` (0-1), with an optional mark.
    Bar {
        label: &'static str,
        fill: f32,
        mark: Option<f32>,
        color: u32,
    },
    Heading(&'static str),
    /// Short dim labels with values, laid out two to a line.
    Grid(Vec<(&'static str, String)>),
    Gap,
}

/// What the person panel says about one agent, top to bottom.
pub(super) fn person_rows(agent: &AgentInspection) -> Vec<Row> {
    let mut rows = vec![Row::Title(labels::person(agent.view.id))];
    if let Some(memory) = &agent.memory {
        let role = if memory.child { "Child" } else { "Founder" };
        rows.push(Row::Text(
            format!("{role} · {}", labels::personality(memory.personality)),
            colors::UI_DIM,
        ));
    }
    rows.push(Row::Gap);

    let (doing, why) = doing(agent);
    rows.push(Row::Text(doing, colors::UI_TEXT));
    if let Some(why) = why {
        rows.push(Row::Text(why, colors::UI_DIM));
    }
    if agent.death.is_some() {
        return rows;
    }
    rows.push(Row::Gap);
    if let Some(health) = agent.health {
        let color = if health.value <= HEALTH_INCAPACITATION_THRESHOLD {
            colors::UI_BAD
        } else if health.value < HEALTH_MAX * 3 / 5 {
            colors::UI_WARN
        } else {
            colors::UI_GOOD
        };
        rows.push(Row::Bar {
            label: "Health",
            fill: f32::from(health.value) / f32::from(HEALTH_MAX),
            mark: None,
            color,
        });
    }
    if let Some(needs) = agent.needs {
        for (label, need) in [
            ("Hunger", needs.hunger),
            ("Thirst", needs.thirst),
            ("Tiredness", needs.rest),
            ("Cold", needs.exposure),
        ] {
            rows.push(need_bar(label, need));
        }
    }
    if let Some(inventory) = agent.inventory {
        let carried: Vec<String> = Material::ALL
            .into_iter()
            .filter(|material| inventory.amount(*material) > 0)
            .map(|material| {
                format!(
                    "{} {}",
                    inventory.amount(material),
                    labels::material(material)
                )
            })
            .collect();
        let carried = if carried.is_empty() {
            "nothing".to_owned()
        } else {
            carried.join(", ")
        };
        rows.push(Row::Pair("Carrying", carried));
    }

    let Some(memory) = &agent.memory else {
        return rows;
    };
    rows.push(Row::Gap);
    rows.push(Row::Heading("What they believe"));
    let before = rows.len();
    let food = |wanted: fn(i32) -> bool| -> Vec<&'static str> {
        Material::ALL
            .into_iter()
            .zip(memory.food)
            .filter(|(_, value)| value.is_some_and(wanted))
            .map(|(material, _)| labels::material(material))
            .collect()
    };
    let animals = |wanted: fn((bool, bool)) -> bool| -> Vec<&'static str> {
        sim_core::Species::ALL
            .into_iter()
            .zip(memory.fauna)
            .filter(|(_, belief)| belief.is_some_and(wanted))
            .map(|(species, _)| labels::species_plural(species))
            .collect()
    };
    for (label, list) in [
        ("Eats", food(|value| value > 0)),
        ("Avoids", food(|value| value < 0)),
        ("Hunts", animals(|(prey, _)| prey)),
        ("Fears", animals(|(_, danger)| danger)),
    ] {
        if !list.is_empty() {
            rows.push(Row::Pair(label, list.join(", ")));
        }
    }
    if memory.knows_hearths {
        rows.push(Row::Text(
            "Knows how to make fire".to_owned(),
            colors::UI_TEXT,
        ));
    }
    if rows.len() == before {
        rows.push(Row::Text("Nothing yet".to_owned(), colors::UI_DIM));
    }

    rows.push(Row::Gap);
    rows.push(Row::Heading("Their words"));
    let words: Vec<(&'static str, String)> = Concept::ALL
        .into_iter()
        .filter_map(|concept| {
            let form = memory.word_for(concept)?;
            Some((labels::concept_short(concept), labels::word(form)))
        })
        .collect();
    if words.is_empty() {
        rows.push(Row::Text("No words yet".to_owned(), colors::UI_DIM));
    } else {
        rows.push(Row::Grid(words));
    }

    rows.push(Row::Gap);
    let known = memory.acquaintances();
    let mut friends: Vec<_> = known
        .iter()
        .filter(|acquaintance| acquaintance.familiarity >= sim_core::FRIEND_FAMILIARITY)
        .collect();
    friends.sort_by_key(|friend| {
        (
            std::cmp::Reverse(friend.familiarity),
            std::cmp::Reverse(friend.trust),
            friend.agent.get(),
        )
    });
    let friends: Vec<String> = friends
        .iter()
        .take(4)
        .map(|friend| labels::person(friend.agent))
        .collect();
    rows.push(Row::Pair(
        "Friends",
        if friends.is_empty() {
            "none yet".to_owned()
        } else {
            friends.join(", ")
        },
    ));
    rows.push(Row::Pair("Knows", format!("{} people", known.len())));
    rows
}

/// What the person is doing, and why, as sentences.
fn doing(agent: &AgentInspection) -> (String, Option<String>) {
    if let Some(death) = agent.death {
        return (capitalized(labels::death(death.cause)), None);
    }
    match agent.view.activity {
        AgentActivity::Sleeping => {
            let place = agent
                .sleep
                .map(|sleep| format!("Sleeping {}", labels::sleep(sleep.quality)));
            return (place.unwrap_or_else(|| "Sleeping".to_owned()), None);
        }
        AgentActivity::Incapacitated => {
            return ("Collapsed, too weak to move".to_owned(), None);
        }
        _ => {}
    }
    match agent.policy {
        Some(policy) => (
            labels::goal(policy.goal).to_owned(),
            labels::reason(policy.reason).map(capitalized),
        ),
        None => (capitalized(labels::activity(agent.view.activity)), None),
    }
}

fn need_bar(label: &'static str, need: NeedLevelView) -> Row {
    let urgent = need
        .threshold
        .saturating_add((NEED_MAX - need.threshold.min(NEED_MAX)) / 2);
    let color = if need.value >= urgent {
        colors::UI_BAD
    } else if need.value >= need.threshold {
        colors::UI_WARN
    } else {
        colors::UI_NEED
    };
    Row::Bar {
        label,
        fill: f32::from(need.value.min(NEED_MAX)) / f32::from(NEED_MAX),
        mark: Some(f32::from(need.threshold.min(NEED_MAX)) / f32::from(NEED_MAX)),
        color,
    }
}

fn capitalized(text: &str) -> String {
    let mut characters = text.chars();
    characters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(characters).collect()
    })
}

/// Lines a row takes at `chars` characters wide.
fn row_lines(row: &Row, chars: usize) -> Vec<String> {
    match row {
        Row::Title(text) | Row::Text(text, _) => wrap(text, chars),
        Row::Pair(_, value) => wrap(value, chars - LABEL_CHARS),
        Row::Heading(text) => vec![(*text).to_owned()],
        Row::Bar { .. } => vec![String::new()],
        Row::Grid(cells) => vec![String::new(); cells.len().div_ceil(2)],
        Row::Gap => Vec::new(),
    }
}

fn row_height(painter: &Painter, row: &Row, chars: usize) -> f32 {
    match row {
        Row::Gap => painter.line() / 2.0,
        _ => row_lines(row, chars).len() as f32 * painter.line(),
    }
}

/// Draws `row` at `y` and returns the y below it.
fn draw_row(painter: &mut Painter, row: &Row, x: f32, y: f32, chars: usize) -> f32 {
    let px = painter.px;
    let line = painter.line();
    match row {
        Row::Gap => return y + line / 2.0,
        Row::Bar {
            label,
            fill,
            mark,
            color,
        } => {
            painter.text(label, x, y, colors::UI_DIM);
            let bar_x = x + painter.chars(LABEL_CHARS);
            let bar_w = painter.chars(chars - LABEL_CHARS);
            let bar_h = 4.0 * px;
            let bar_y = y + 2.5 * px;
            painter.rect(bar_x, bar_y, bar_w, bar_h, colors::UI_TRACK);
            painter.rect(bar_x, bar_y, bar_w * fill.clamp(0.0, 1.0), bar_h, *color);
            if let Some(mark) = mark {
                painter.rect(
                    bar_x + bar_w * mark.clamp(0.0, 1.0),
                    bar_y - px,
                    (px / 2.0).max(1.0),
                    bar_h + 2.0 * px,
                    colors::UI_DIM,
                );
            }
            return y + line;
        }
        Row::Grid(cells) => {
            let half = chars / 2;
            let mut cell_y = y;
            for pair in cells.chunks(2) {
                for (column, (label, value)) in pair.iter().enumerate() {
                    let cell_x = x + painter.chars(column * half);
                    painter.text(label, cell_x, cell_y, colors::UI_DIM);
                    painter.text(
                        value,
                        cell_x + painter.chars(GRID_LABEL_CHARS),
                        cell_y,
                        colors::UI_TEXT,
                    );
                }
                cell_y += line;
            }
            return cell_y;
        }
        _ => {}
    }
    let (text_x, color) = match row {
        Row::Title(_) => (x, colors::UI_TITLE),
        Row::Text(_, color) => (x, *color),
        Row::Pair(label, _) => {
            painter.text(label, x, y, colors::UI_DIM);
            (x + painter.chars(LABEL_CHARS), colors::UI_TEXT)
        }
        Row::Heading(_) => (x, colors::UI_ACCENT),
        Row::Bar { .. } | Row::Grid(_) | Row::Gap => unreachable!("handled above"),
    };
    let mut line_y = y;
    for text in row_lines(row, chars) {
        painter.text(&text, text_x, line_y, color);
        line_y += line;
    }
    line_y
}

fn person_panel(
    painter: &mut Painter,
    agent: &AgentInspection,
    following: bool,
    width: f32,
    height: f32,
    top: f32,
) {
    let rows = person_rows(agent);
    let pad = painter.pad();
    let line = painter.line();
    let inner = painter.chars(PANEL_CHARS);
    let panel_w = inner + 2.0 * pad;
    let x = (width - pad - panel_w).max(0.0);
    let y = top + pad;
    let content: f32 = rows
        .iter()
        .map(|row| row_height(painter, row, PANEL_CHARS))
        .sum();
    let legend = wrap(
        "On the map: squares are places they remember, dots lead to people they know",
        PANEL_CHARS,
    );
    let button_h = line + 2.0 * painter.px;
    let footer = pad / 2.0 + legend.len() as f32 * line + 2.0 * painter.px + button_h + pad;
    let panel_h = (2.0 * pad + content + footer).min(height - y - pad);
    painter.panel(x, y, panel_w, panel_h);

    let close_w = painter.button_width("×");
    painter.button(
        "×",
        x + panel_w - pad - close_w,
        y + pad - painter.px,
        UiAction::CloseSelected,
        false,
    );

    let bottom = y + panel_h - footer;
    let mut row_y = y + pad;
    for row in &rows {
        if row_y + row_height(painter, row, PANEL_CHARS) > bottom {
            break;
        }
        row_y = draw_row(painter, row, x + pad, row_y, PANEL_CHARS);
    }

    let mut footer_y = bottom + pad / 2.0;
    for text in &legend {
        painter.text(text, x + pad, footer_y, colors::UI_DIM);
        footer_y += line;
    }
    let button_y = y + panel_h - pad - button_h;
    let follow = if following { "F Following" } else { "F Follow" };
    let follow_w = painter.button(follow, x + pad, button_y, UiAction::Follow, following);
    painter.button(
        "Tab Next person",
        x + pad + follow_w + 2.0 * painter.px,
        button_y,
        UiAction::NextPerson,
        false,
    );
}

/// The F3 readout, top left under the bar.
fn details_panel(painter: &mut Painter, text: &str, top: f32) {
    let pad = painter.pad();
    let line = painter.line();
    let chars = text
        .lines()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    let lines = text.lines().count();
    let x = pad;
    let y = top + pad;
    painter.panel(
        x,
        y,
        painter.chars(chars) + 2.0 * pad,
        lines as f32 * line + 2.0 * pad,
    );
    for (index, text) in text.lines().enumerate() {
        painter.text(
            text,
            x + pad,
            y + pad + index as f32 * line,
            colors::UI_TEXT,
        );
    }
}

/// Recent events, newest at the bottom, above `bottom`.
fn feed(painter: &mut Painter, state: &RenderState, bottom: f32) {
    if state.feed.is_empty() {
        return;
    }
    let pad = painter.pad();
    let line = painter.line();
    let entries: Vec<(usize, Vec<String>, Tone)> = state
        .feed
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let text = match entry.count {
                1 => entry.text.clone(),
                count => format!("{} ×{count}", entry.text),
            };
            (index, wrap(&text, FEED_CHARS - 2), entry.tone)
        })
        .collect();
    let lines: usize = entries.iter().map(|(_, lines, _)| lines.len()).sum();
    let w = painter.chars(FEED_CHARS) + 2.0 * pad;
    let h = (lines + 1) as f32 * line + 2.0 * pad;
    let x = pad;
    let y = bottom - pad - h;
    painter.panel(x, y, w, h);
    painter.text(
        "Recent events · click one to look",
        x + pad,
        y + pad,
        colors::UI_DIM,
    );
    let mut entry_y = y + pad + line;
    for (index, lines, tone) in entries {
        let entry_h = lines.len() as f32 * line;
        if painter.hovered(x, entry_y, w, entry_h) {
            painter.rect(
                x + px_half(painter),
                entry_y,
                w - 2.0 * px_half(painter),
                entry_h,
                colors::UI_BUTTON_HOVER,
            );
        }
        let size = 4.0 * painter.px;
        painter.rect(
            x + pad,
            entry_y + 3.0 * painter.px,
            size,
            size,
            tone_color(tone),
        );
        for (offset, text) in lines.iter().enumerate() {
            painter.text(
                text,
                x + pad + painter.chars(2),
                entry_y + offset as f32 * line,
                colors::UI_TEXT,
            );
        }
        painter.hits.push(Hit {
            x,
            y: entry_y,
            width: w,
            height: entry_h,
            action: Some(UiAction::FeedEntry(index)),
        });
        entry_y += entry_h;
    }
}

fn px_half(painter: &Painter) -> f32 {
    (painter.px / 2.0).max(1.0)
}

const fn tone_color(tone: Tone) -> u32 {
    match tone {
        Tone::Bad => colors::UI_BAD,
        Tone::Good => colors::UI_GOOD,
        Tone::Talk => colors::UI_TALK,
        Tone::Neutral => colors::UI_DIM,
    }
}

/// The palette of things to place; returns its top edge.
fn build_palette(painter: &mut Painter, selected: BuildTool, width: f32, height: f32) -> f32 {
    let pad = painter.pad();
    let line = painter.line();
    let gap = 2.0 * painter.px;
    let labels: Vec<(BuildTool, String)> = BuildTool::ALL
        .into_iter()
        .map(|tool| (tool, tool.label().to_owned()))
        .collect();
    let buttons_w: f32 = labels
        .iter()
        .map(|(_, label)| painter.button_width(label) + gap)
        .sum::<f32>()
        - gap;
    let hint = "Click the map to place · B or Esc to stop";
    let w = buttons_w.max(text_width(hint, painter.px)) + 2.0 * pad;
    let h = 2.0 * line + 2.0 * painter.px + 2.0 * pad;
    let x = ((width - w) / 2.0).max(0.0);
    let y = height - pad - h;
    painter.panel(x, y, w, h);
    let mut button_x = x + (w - buttons_w) / 2.0;
    for (tool, label) in &labels {
        button_x += painter.button(
            label,
            button_x,
            y + pad,
            UiAction::Tool(*tool),
            *tool == selected,
        ) + gap;
    }
    let hint_x = x + (w - text_width(hint, painter.px)) / 2.0;
    painter.text(
        hint,
        hint_x,
        y + pad + line + 3.0 * painter.px,
        colors::UI_DIM,
    );
    y
}

/// Words being said right now, over the speakers' heads.
fn speech_bubbles(painter: &mut Painter, view: CameraView, gestures: &[GestureMark]) {
    let (width, height) = view.screen_size();
    let scale = view.scale() as f32;
    if scale < MIN_BUBBLE_CELL_PIXELS {
        return;
    }
    let [center_x, center_y] = view.center();
    let px = painter.px;
    // Newest first: one bubble per speaker, and none on top of another.
    let mut speakers = Vec::new();
    let mut drawn: Vec<(f32, f32, f32, f32)> = Vec::new();
    for gesture in gestures.iter().rev() {
        if speakers.contains(&gesture.sender) {
            continue;
        }
        speakers.push(gesture.sender);
        let mut text = gesture
            .word
            .map(|form| {
                let name = form.name();
                if gesture.loud {
                    format!("{}!", name.to_uppercase())
                } else {
                    name
                }
            })
            .unwrap_or_default();
        let mime = labels::mime(gesture.mime);
        if text.is_empty() {
            text = format!("({mime})");
        } else {
            text = format!("{text} ({mime})");
        }
        let anchor_x = (gesture.origin.x as f32 + 0.5 - center_x) * scale + width / 2.0;
        let anchor_y = (gesture.origin.y as f32 - center_y) * scale + height / 2.0;
        let w = text_width(&text, px) + 2.0 * 2.0 * px;
        let h = painter.line();
        let x = anchor_x - w / 2.0;
        let y = anchor_y - h - 3.0 * px;
        if x + w < 0.0 || y + h < 0.0 || x > width || y > height {
            continue;
        }
        if drawn
            .iter()
            .any(|&(dx, dy, dw, dh)| x < dx + dw && dx < x + w && y < dy + dh && dy < y + h)
        {
            continue;
        }
        drawn.push((x, y, w, h));
        let color = if gesture.loud {
            colors::UI_BUBBLE_LOUD
        } else {
            colors::UI_BUBBLE
        };
        painter.rect(x, y, w, h, color);
        painter.rect(anchor_x - px, y + h, 2.0 * px, 2.0 * px, color);
        painter.text(&text, x + 2.0 * px, y - px / 2.0, rgba(20, 22, 26, 255));
    }
}

/// Cells must be at least this many pixels wide before words are drawn.
const MIN_BUBBLE_CELL_PIXELS: f32 = 2.5;

/// Controls on the left, the map legend on the right.
fn help(painter: &mut Painter, width: f32, height: f32) {
    painter.rect(0.0, 0.0, width, height, colors::UI_SCRIM);
    painter.hits.push(Hit {
        x: 0.0,
        y: 0.0,
        width,
        height,
        action: Some(UiAction::Help),
    });
    let pad = painter.pad();
    let line = painter.line();
    let px = painter.px;
    let column = 36;
    let intro = wrap(
        "Nobody here starts with a language. Each person learns for themselves what is safe \
         to eat, which animals are dangerous, and how to make fire, and they invent words for \
         things. Words can be misunderstood, and both sides learn from what happens next.",
        column * 2 + 2,
    );
    const CONTROLS: [(&str, &str); 13] = [
        ("Drag", "move the map"),
        ("Scroll", "zoom in and out"),
        ("Click", "pick a person"),
        ("Tab", "next person"),
        ("F", "follow the picked person"),
        ("Space", "pause or resume"),
        ("1-9  + -", "speed"),
        ("H", "this help"),
        ("Esc", "close what's open"),
        ("T", "add a person at the mouse"),
        ("B", "place trees, bushes, rocks"),
        ("Shift+R", "remove everyone"),
        ("F3", "technical details"),
    ];
    let legend: [(&str, &[(u32, &str)]); 9] = [
        (
            "People",
            &[
                (colors::agent_color(AgentActivity::Moving), "walking"),
                (colors::agent_color(AgentActivity::Gathering), "gathering"),
            ],
        ),
        (
            "",
            &[
                (colors::agent_color(AgentActivity::Building), "building"),
                (colors::agent_color(AgentActivity::Sleeping), "sleeping"),
            ],
        ),
        (
            "",
            &[
                (colors::agent_color(AgentActivity::Idle), "idle"),
                (colors::agent_color(AgentActivity::Dead), "dead"),
            ],
        ),
        ("Animals", &[(colors::DEER, "deer"), (colors::WOLF, "wolf")]),
        ("", &[(colors::CARCASS, "carcass (meat)")]),
        (
            "Built",
            &[(colors::HUT, "hut"), (colors::HEARTH, "hearth (fire)")],
        ),
        ("", &[(colors::UNDER_CONSTRUCTION, "being built")]),
        (
            "Nature",
            &[
                (colors::feature_color(sim_core::FeatureKind::Tree), "tree"),
                (colors::feature_color(sim_core::FeatureKind::Rock), "rock"),
            ],
        ),
        (
            "",
            &[
                (
                    colors::feature_color(sim_core::FeatureKind::BerryBush),
                    "berries",
                ),
                (
                    colors::feature_color(sim_core::FeatureKind::BitterBush),
                    "bitter berries",
                ),
            ],
        ),
    ];
    let notes: Vec<String> = [
        "Bitter berries look like food but make you sick.",
        "A bubble is a word being said, with the gesture that went with it. A dotted line is someone pointing.",
    ]
    .iter()
    .flat_map(|note| wrap(note, column))
    .collect();
    let pad = 2.0 * pad;
    let column_w = painter.chars(column);
    let w = (2.0 * column_w + painter.chars(2) + 2.0 * pad).min(width);
    let body_rows = CONTROLS.len().max(legend.len() + notes.len()) as f32 + 0.5;
    let h = (2.0 * pad + line * (1.5 + intro.len() as f32 + 0.5 + 1.0 + body_rows + 1.0 + 1.0))
        .min(height);
    let x = ((width - w) / 2.0).max(0.0);
    let y = ((height - h) / 2.0).max(0.0);
    painter.panel(x, y, w, h);
    let left = x + pad;
    let right = left + column_w + painter.chars(2);
    let mut row_y = y + pad;
    painter.text("Let There Be Life", left, row_y, colors::UI_TITLE);
    row_y += line * 1.5;
    for text in &intro {
        painter.text(text, left, row_y, colors::UI_TEXT);
        row_y += line;
    }
    row_y += line / 2.0;
    painter.text("Controls", left, row_y, colors::UI_ACCENT);
    painter.text("On the map", right, row_y, colors::UI_ACCENT);
    row_y += line;
    let body_y = row_y;
    for (key, action) in CONTROLS {
        painter.text(key, left, row_y, colors::UI_TEXT);
        painter.text(action, left + painter.chars(10), row_y, colors::UI_DIM);
        row_y += line;
    }
    row_y = body_y;
    for (group, items) in legend {
        painter.text(group, right, row_y, colors::UI_DIM);
        let mut item_x = right + painter.chars(8);
        for (color, label) in items {
            let size = 5.0 * px;
            painter.rect(item_x, row_y + 2.0 * px, size, size, *color);
            item_x = painter.text(
                label,
                item_x + painter.chars(1) + size - px,
                row_y,
                colors::UI_TEXT,
            );
            item_x += painter.chars(2);
        }
        row_y += line;
    }
    row_y += line / 2.0;
    for note in &notes {
        painter.text(note, right, row_y, colors::UI_DIM);
        row_y += line;
    }
    let close = "Press H or Esc, or click outside, to close";
    painter.text(
        close,
        x + (w - text_width(close, px)) / 2.0,
        y + h - pad - line,
        colors::UI_DIM,
    );
}
