//! Mouse and keyboard handling: what is under the cursor, zoom, map clicks,
//! interface buttons, and key commands.

use std::time::Instant;

use sim_core::{AgentActivity, AgentView, AnimalView, EngineCommand, Material, WorldPosition};
use winit::{event::MouseScrollDelta, event_loop::ActiveEventLoop, keyboard::KeyCode};

use super::{PICK_PIXELS, ViewerApp};
use crate::{
    camera::Viewport,
    labels,
    render::{BuildTool, Hover, SPEEDS, UiAction},
    startup::VIEWER_AGENT_LIMIT,
};

/// Zoomed out further than this (pixels per cell), focusing on something zooms in.
const FOCUS_MIN_CELL_PIXELS: f64 = 3.0;
/// Cells across the shorter screen axis after zooming in to focus.
const FOCUS_SPAN_CELLS: f64 = 96.0;

impl ViewerApp {
    pub(super) fn update_hover(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let size = window.inner_size();
        let position = self.cursor.map(|(x, y)| {
            self.camera
                .screen_to_world_position(x, y, self.viewport(size.width, size.height))
        });
        self.cursor_world = position;
        self.inspected =
            position.and_then(|position| self.engine.world().inspect_chunk_at(position).ok());
        self.hovered = position.filter(|position| self.engine.world().cell(*position).is_some());
        self.hover = if self.cursor_over_ui() {
            None
        } else {
            self.hovered.and_then(|position| self.describe(position))
        };
    }

    fn cursor_over_ui(&self) -> bool {
        let (Some((x, y)), Some(renderer)) = (self.cursor, &self.renderer) else {
            return false;
        };
        renderer.ui_at(x, y).is_some()
    }

    /// How many cells from the cursor still count as "under" it.
    fn pick_radius(&self) -> i64 {
        let Some(window) = &self.window else {
            return 0;
        };
        let size = window.inner_size();
        let viewport = self.viewport(size.width, size.height);
        let scale = self
            .camera
            .view(
                viewport.screen_width,
                viewport.screen_height,
                viewport.world_width,
                viewport.world_height,
            )
            .scale();
        (PICK_PIXELS / scale.max(f64::EPSILON)).floor().min(64.0) as i64
    }

    /// The person nearest `position` within the pick radius, living ones first.
    fn person_near(&self, position: WorldPosition) -> Option<AgentView> {
        let radius = self.pick_radius();
        self.engine
            .agent_views(VIEWER_AGENT_LIMIT)
            .filter(|agent| near(agent.position, position, radius))
            .min_by_key(|agent| {
                (
                    agent.activity == AgentActivity::Dead,
                    distance2(agent.position, position),
                    agent.id.get(),
                )
            })
    }

    fn animal_near(&self, position: WorldPosition) -> Option<AnimalView> {
        let radius = self.pick_radius();
        self.engine
            .animal_views()
            .filter(|animal| near(animal.position, position, radius))
            .min_by_key(|animal| (distance2(animal.position, position), animal.id))
    }

    /// What the tooltip says is at `position`.
    fn describe(&self, position: WorldPosition) -> Option<Hover> {
        if let Some(agent) = self.person_near(position) {
            return Some(Hover::Person {
                id: agent.id,
                name: self.engine.life(agent.id).map(|life| life.name),
                activity: agent.activity,
            });
        }
        if let Some(animal) = self.animal_near(position) {
            return Some(Hover::Animal {
                species: animal.species,
                mode: animal.mode,
            });
        }
        if let Some(structure) = self
            .engine
            .structure_views(VIEWER_AGENT_LIMIT)
            .find(|structure| structure.position == position)
        {
            return Some(Hover::Structure {
                kind: structure.kind,
                state: structure.state,
            });
        }
        let world = self.engine.world();
        let spawned = self.engine.spawned_object_at(position);
        if let Ok(Some(resource)) = self.engine.available_resource_at(position) {
            if resource.kind == Material::Meat {
                return Some(Hover::Carcass {
                    meat: resource.capacity,
                });
            }
            let label = spawned
                .map(|object| labels::spawn_kind(object.kind))
                .or_else(|| {
                    world
                        .feature_at(position)
                        .map(|feature| labels::feature(feature.kind))
                })
                .unwrap_or(labels::material(resource.kind));
            return Some(Hover::Resource {
                label,
                remaining: Some((resource.capacity, resource.kind)),
            });
        }
        if let Some(object) = spawned {
            return Some(Hover::Resource {
                label: labels::spawn_kind(object.kind),
                remaining: None,
            });
        }
        world
            .cell(position)
            .map(|cell| Hover::Terrain(labels::terrain(cell.surface(), cell.biome())))
    }

    pub(super) fn zoom(&mut self, delta: MouseScrollDelta) {
        let (Some(window), Some((cursor_x, cursor_y))) = (&self.window, self.cursor) else {
            return;
        };
        let steps = match delta {
            MouseScrollDelta::LineDelta(_, lines) => f64::from(lines),
            MouseScrollDelta::PixelDelta(position) => position.y / 120.0,
        };
        let size = window.inner_size();
        self.camera.zoom_at(
            steps,
            (cursor_x, cursor_y),
            Viewport {
                screen_width: size.width,
                screen_height: size.height,
                world_width: self.engine.world().width(),
                world_height: self.engine.world().height(),
            },
        );
        self.update_hover();
    }

    /// A left click that did not drag, on the map (not the interface).
    pub(super) fn click_map(&mut self) {
        match self.build {
            Some(BuildTool::Person) => self.spawn_agent_at_cursor(),
            Some(BuildTool::Object(kind)) => self.spawn_object_at_cursor(kind),
            None => {
                let picked = self
                    .hovered
                    .and_then(|position| self.person_near(position))
                    .map(|agent| agent.id);
                if picked != self.selected {
                    self.following = false;
                }
                self.selected = picked;
            }
        }
    }

    pub(super) fn apply_ui_action(&mut self, action: UiAction) {
        match action {
            UiAction::TogglePause => self.engine_command(EngineCommand::TogglePause),
            UiAction::Slower => self.step_speed(-1),
            UiAction::Faster => self.step_speed(1),
            UiAction::Help => self.help_open = !self.help_open,
            UiAction::CloseSelected => {
                self.selected = None;
                self.following = false;
            }
            UiAction::Follow => self.following = !self.following && self.selected.is_some(),
            UiAction::NextPerson => self.select_next_person(),
            UiAction::Tool(tool) => self.build = Some(tool),
            UiAction::FeedEntry(index) => {
                let Some(entry) = self.feed.entries().nth(index).cloned() else {
                    return;
                };
                if let Some(agent) = entry.agent {
                    self.selected = Some(agent);
                    self.following = false;
                }
                self.focus_on(entry.position);
            }
        }
    }

    fn step_speed(&mut self, direction: isize) {
        let speed = self.engine.snapshot().speed;
        let current = SPEEDS
            .iter()
            .position(|step| *step >= speed)
            .unwrap_or(SPEEDS.len() - 1);
        let next = current
            .saturating_add_signed(direction)
            .min(SPEEDS.len() - 1);
        self.engine_command(EngineCommand::SetSpeed(SPEEDS[next]));
    }

    /// Picks the next living person after the selected one (by id) and looks at them.
    fn select_next_person(&mut self) {
        let after = self.selected.map_or(0, |agent| agent.get());
        let living: Vec<AgentView> = self
            .engine
            .agent_views(VIEWER_AGENT_LIMIT)
            .filter(|agent| agent.activity != AgentActivity::Dead)
            .collect();
        let Some(next) = living
            .iter()
            .find(|agent| agent.id.get() > after)
            .or_else(|| living.first())
            .copied()
        else {
            self.show_toast("Nobody is alive");
            return;
        };
        self.selected = Some(next.id);
        self.focus_on(next.position);
    }

    /// Centers the map on `position`, zooming in if it is too far out to see.
    pub(super) fn focus_on(&mut self, position: WorldPosition) {
        let Some(window) = &self.window else {
            return;
        };
        let size = window.inner_size();
        let viewport = self.viewport(size.width, size.height);
        let scale = self
            .camera
            .view(
                viewport.screen_width,
                viewport.screen_height,
                viewport.world_width,
                viewport.world_height,
            )
            .scale();
        if scale < FOCUS_MIN_CELL_PIXELS {
            self.camera.focus_on(position, FOCUS_SPAN_CELLS, viewport);
        } else {
            self.camera.center_on(position, viewport);
        }
        self.update_hover();
    }

    fn engine_command(&mut self, command: EngineCommand) {
        self.engine.command(command);
        if matches!(
            command,
            EngineCommand::TogglePause | EngineCommand::SetPaused(_)
        ) {
            self.last_frame = Some(Instant::now());
            self.accumulator = 0.0;
            self.next_frame = Instant::now();
        }
    }

    pub(super) fn handle_key(&mut self, code: KeyCode, _event_loop: &ActiveEventLoop) {
        match code {
            KeyCode::Space => self.engine_command(EngineCommand::TogglePause),
            KeyCode::Digit1
            | KeyCode::Digit2
            | KeyCode::Digit3
            | KeyCode::Digit4
            | KeyCode::Digit5
            | KeyCode::Digit6
            | KeyCode::Digit7
            | KeyCode::Digit8
            | KeyCode::Digit9 => {
                let index = digit_index(code);
                self.engine_command(EngineCommand::SetSpeed(SPEEDS[index]));
            }
            KeyCode::Equal | KeyCode::NumpadAdd => self.step_speed(1),
            KeyCode::Minus | KeyCode::NumpadSubtract => self.step_speed(-1),
            KeyCode::KeyH | KeyCode::F1 => self.help_open = !self.help_open,
            KeyCode::F3 => self.details_open = !self.details_open,
            KeyCode::Escape => {
                if self.help_open {
                    self.help_open = false;
                } else if self.build.is_some() {
                    self.build = None;
                } else {
                    self.selected = None;
                    self.following = false;
                }
            }
            KeyCode::KeyF => self.apply_ui_action(UiAction::Follow),
            KeyCode::Tab => self.select_next_person(),
            KeyCode::KeyB => {
                self.build = match self.build {
                    Some(_) => None,
                    None => Some(BuildTool::Object(sim_core::SpawnKind::BerryBush)),
                };
            }
            KeyCode::KeyT => self.spawn_agent_at_cursor(),
            KeyCode::KeyR if self.shift => self.reset_population(),
            KeyCode::KeyR => self.show_toast("Press Shift+R to remove everyone"),
            KeyCode::KeyC => self.cancel_pending_generation(),
            _ => {}
        }
    }
}

const fn digit_index(code: KeyCode) -> usize {
    match code {
        KeyCode::Digit2 => 1,
        KeyCode::Digit3 => 2,
        KeyCode::Digit4 => 3,
        KeyCode::Digit5 => 4,
        KeyCode::Digit6 => 5,
        KeyCode::Digit7 => 6,
        KeyCode::Digit8 => 7,
        KeyCode::Digit9 => 8,
        _ => 0,
    }
}

fn near(a: WorldPosition, b: WorldPosition, radius: i64) -> bool {
    (a.x - b.x).abs() <= radius && (a.y - b.y).abs() <= radius
}

fn distance2(a: WorldPosition, b: WorldPosition) -> i64 {
    let (dx, dy) = (a.x - b.x, a.y - b.y);
    dx * dx + dy * dy
}
