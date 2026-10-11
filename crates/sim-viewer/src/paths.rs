//! Presentation-only wear on the ground: how often people have stepped on each
//! cell lately, so worn paths show where routines run. Never read by the simulation.

use std::collections::BTreeMap;

use sim_core::{AgentActivity, Engine, WorldPosition, WorldRect};

/// Ticks between halvings of the wear, so old paths fade (an eighth of a year).
const FADE_TICKS: u64 = sim_core::TICKS_PER_YEAR / 8;
/// Most wear a cell keeps.
const MAX_WEAR: u16 = 400;
/// Cells worn into paths this many steps or more show.
pub const SHOWN_WEAR: u16 = 6;

#[derive(Debug, Default)]
pub struct PathWear {
    wear: BTreeMap<(i64, i64), u16>,
    /// Where each person stood at the last tick, by id.
    last: Vec<Option<WorldPosition>>,
}

impl PathWear {
    /// Notes every step taken in the latest tick.
    pub fn record_tick(&mut self, engine: &Engine) {
        for agent in engine.agent_views(usize::MAX) {
            let index = agent.id.get() as usize;
            if self.last.len() <= index {
                self.last.resize(index + 1, None);
            }
            let moved = self.last[index].is_some_and(|last| last != agent.position);
            self.last[index] = (agent.activity != AgentActivity::Dead).then_some(agent.position);
            if moved {
                let wear = self
                    .wear
                    .entry((agent.position.x, agent.position.y))
                    .or_default();
                *wear = (*wear + 1).min(MAX_WEAR);
            }
        }
        if engine.snapshot().tick % FADE_TICKS == 0 {
            self.wear.retain(|_, wear| {
                *wear /= 2;
                *wear > 0
            });
        }
    }

    /// Worn cells in `bounds` and how worn they are.
    pub fn worn_in(&self, bounds: WorldRect) -> impl Iterator<Item = (WorldPosition, u16)> + '_ {
        // Keys run by x then y: only the columns in view are scanned.
        self.wear
            .range((bounds.min.x, i64::MIN)..(bounds.max.x, i64::MIN))
            .filter(move |&(&(x, y), &wear)| {
                wear >= SHOWN_WEAR && bounds.contains(WorldPosition { x, y })
            })
            .map(|(&(x, y), &wear)| (WorldPosition { x, y }, wear))
    }

    pub fn clear(&mut self) {
        self.wear.clear();
        self.last.clear();
    }
}
