//! Presentation-only: where each person stepped from, so they're drawn gliding
//! between cells, and which family line they belong to (for their color).
//! Never read by the simulation.

use sim_core::{AgentId, Engine, FamilyEvent, WorldPosition};

/// Ticks a step is drawn over (steps take about this long on open ground).
const GLIDE_TICKS: f32 = 16.0;

#[derive(Debug, Clone, Copy)]
struct Step {
    from: WorldPosition,
    to: WorldPosition,
    at: u64,
}

#[derive(Debug, Default)]
pub struct Motion {
    steps: Vec<Option<Step>>,
    /// Family lines of newcomer couples, after the founding families' lines.
    newcomers: Vec<(AgentId, u8)>,
}

impl Motion {
    /// Notes the latest tick's steps and newcomers.
    pub fn record_tick(&mut self, engine: &Engine) {
        let tick = engine.snapshot().tick;
        for agent in engine.agent_views(usize::MAX) {
            let index = agent.id.get() as usize;
            if self.steps.len() <= index {
                self.steps.resize(index + 1, None);
            }
            match &mut self.steps[index] {
                Some(step) if step.to == agent.position => {}
                slot => {
                    let from = slot.map_or(agent.position, |step| step.to);
                    *slot = Some(Step {
                        from,
                        to: agent.position,
                        at: tick,
                    });
                }
            }
        }
        for event in engine.family_events() {
            if let FamilyEvent::Arrived { woman, man } = *event {
                let line = 2 + (self.newcomers.len() / 2) as u8;
                self.newcomers.push((woman, line));
                self.newcomers.push((man, line));
            }
        }
    }

    /// Where to draw someone standing at `position`: partway along their last
    /// step while it's fresh, and whether they're mid-stride (for the gait).
    pub fn drawn(&self, agent: AgentId, position: WorldPosition, tick: u64) -> ([f32; 2], bool) {
        let at = [position.x as f32, position.y as f32];
        let Some(Some(step)) = self.steps.get(agent.get() as usize) else {
            return (at, false);
        };
        let far = step.from.x.abs_diff(step.to.x) + step.from.y.abs_diff(step.to.y) > 1;
        if step.to != position || far {
            return (at, false);
        }
        let progress = (tick.saturating_sub(step.at) as f32 / GLIDE_TICKS).min(1.0);
        if progress >= 1.0 {
            return (at, false);
        }
        let lerp = |from: i64, to: i64| from as f32 + (to - from) as f32 * progress;
        (
            [lerp(step.from.x, step.to.x), lerp(step.from.y, step.to.y)],
            progress > 0.5,
        )
    }

    /// Which family line someone belongs to: a founding family (0 or 1, by
    /// their founder), a newcomer couple (2 and up), passed down from parent to
    /// child; none if unknown.
    pub fn family_line(&self, engine: &Engine, agent: AgentId) -> Option<u8> {
        let mut person = agent;
        // Follow parents back to someone with no known parent.
        for _ in 0..32 {
            match engine.parent_of(person) {
                Some(parent) if parent != person => person = parent,
                _ => break,
            }
        }
        if let Some(&(_, line)) = self
            .newcomers
            .iter()
            .find(|(newcomer, _)| *newcomer == person)
        {
            return Some(line);
        }
        let founders = 2 * sim_core::FAMILY_SIZE;
        (person.get() < founders).then(|| (person.get() / sim_core::FAMILY_SIZE) as u8)
    }

    pub fn clear(&mut self) {
        self.steps.clear();
        self.newcomers.clear();
    }
}
