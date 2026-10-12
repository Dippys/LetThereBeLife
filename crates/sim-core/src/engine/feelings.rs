//! What happens to people shapes them (docs/plans/FEELINGS.md). Once a tick,
//! after everything has happened, this reads what each person went through
//! (bites, plans that worked or failed, tips that proved right or wrong,
//! losses) and lets it move their traits a little and stir their feelings.

use super::Engine;
use crate::cognition::PlanEventKind;

/// Trait indices, in `Personality::traits` order.
const WARMTH: usize = 5;
const BOLDNESS: usize = 4;
const TEMPER: usize = 6;
const STEADINESS: usize = 7;

impl Engine {
    pub(super) fn feel(&mut self) {
        let mut nudges = Vec::new();
        for event in &self.wildlife_events {
            if let crate::WildlifeEvent::Bite { agent, .. } = *event {
                nudges.push((agent, BOLDNESS, -3));
            }
        }
        for event in &self.plan_events {
            match event.kind {
                PlanEventKind::Completed => nudges.push((event.agent, BOLDNESS, 1)),
                PlanEventKind::Revised => nudges.push((event.agent, STEADINESS, -1)),
                PlanEventKind::Formed => {}
            }
        }
        for outcome in &self.hint_outcomes {
            if outcome.teller.is_some() {
                if outcome.confirmed {
                    nudges.push((outcome.agent, WARMTH, 1));
                } else {
                    nudges.push((outcome.agent, TEMPER, 1));
                    nudges.push((outcome.agent, WARMTH, -1));
                }
            }
        }
        for grief in &self.grief_events {
            nudges.push((grief.agent, STEADINESS, -2));
        }
        for (agent, index, amount) in nudges {
            self.minds.get_mut(agent).character.nudge(index, amount);
        }
        // Bites frighten (the timid more); losing someone close grieves.
        let mut felt = Vec::new();
        for event in &self.wildlife_events {
            if let crate::WildlifeEvent::Bite { agent, .. } = *event {
                felt.push((agent, crate::Feeling::Fear, 160));
            }
        }
        for grief in &self.grief_events {
            felt.push((grief.agent, crate::Feeling::Grief, 255));
        }
        // A plan that failed frustrates; retching disgusts.
        for event in &self.plan_events {
            if event.kind == PlanEventKind::Revised {
                felt.push((event.agent, crate::Feeling::Frustration, 40));
            }
        }
        for meal in &self.meal_events {
            if meal.retched {
                felt.push((meal.agent, crate::Feeling::Disgust, 120));
            }
        }
        for (agent, feeling, amount) in felt {
            let boldness = u32::from(self.personality_in_use(agent).boldness);
            let amount = if feeling == crate::Feeling::Fear {
                (amount * (320 - boldness) / 192).min(255)
            } else {
                amount
            };
            self.minds
                .get_mut(agent)
                .feelings
                .feel(feeling, amount as u8);
        }
    }

    /// Once a year: skill not kept up by practice fades, and grudges halve.
    pub(super) fn fade_skills(&mut self) {
        let living: Vec<crate::AgentId> = self
            .population
            .views(usize::MAX)
            .filter(|view| view.activity != crate::AgentActivity::Dead)
            .map(|view| view.id)
            .collect();
        for agent in living {
            if self.minds.get(agent).is_some() {
                let mind = self.minds.get_mut(agent);
                mind.character.fade();
                mind.social.cool_off();
            }
        }
    }
}
