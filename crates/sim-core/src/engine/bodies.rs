//! The dead: bodies lie where people died until someone buries them. Left
//! out, a body rots after a day and sickens whoever stays close by; after a
//! season only bones are left, and after a year nothing. Family and close
//! friends bury their own; anyone who has fallen sick by a rotting body, or
//! watched a burial, buries any body.

use super::Engine;
use crate::{AgentId, NeedKind, WorldPosition};

/// A body starts to rot after this long (one day).
const ROT_TICKS: u64 = crate::DAY_SECONDS * 60;
/// Only bones are left after this long (a season).
const BONES_TICKS: u64 = crate::SEASON_TICKS;
/// Bones are gone after this long (a year).
const GONE_TICKS: u64 = crate::TICKS_PER_YEAR;
/// A rotting body sickens people this close (cells).
const MIASMA_REACH: u64 = 2;
/// How much a rotting body's air adds to thirst and to tiredness.
const MIASMA: u16 = 1_500;
/// Someone falls sick from it at most once a day.
const SICK_GAP_SECONDS: u32 = crate::DAY_SECONDS as u32;

/// What's left where someone died.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyState {
    Fresh,
    Rotting,
    Bones,
    Buried,
}

/// Someone buried a body (latest tick, for logs and tools).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BurialEvent {
    pub agent: AgentId,
    pub body: AgentId,
    pub at: WorldPosition,
    /// How many saw it done (and learned to).
    pub watchers: u16,
}

impl Engine {
    /// Every body still to be seen: who, where, and what state it's in
    /// (graves stay).
    pub fn bodies(&self) -> impl Iterator<Item = (AgentId, WorldPosition, BodyState)> + '_ {
        self.death_records
            .iter()
            .filter_map(|record| Some((record.agent, record.position, self.body_state(record)?)))
    }

    fn body_state(&self, record: &crate::DeathRecord) -> Option<BodyState> {
        if self.buried.contains(&record.agent) {
            return Some(BodyState::Buried);
        }
        let age = self.time.ticks().saturating_sub(record.at.ticks());
        match age {
            _ if age >= GONE_TICKS => None,
            _ if age >= BONES_TICKS => Some(BodyState::Bones),
            _ if age >= ROT_TICKS => Some(BodyState::Rotting),
            _ => Some(BodyState::Fresh),
        }
    }

    /// Unburied bodies inside `area`: who and where.
    pub(super) fn unburied_in(&self, area: crate::WorldRect) -> Vec<(AgentId, WorldPosition)> {
        self.death_records
            .iter()
            .filter(|record| area.contains(record.position))
            .filter(|record| {
                matches!(
                    self.body_state(record),
                    Some(BodyState::Fresh | BodyState::Rotting | BodyState::Bones)
                )
            })
            .map(|record| (record.agent, record.position))
            .collect()
    }

    /// Near a rotting body, `agent` falls sick (at most once a day), and
    /// learns that the dead are better buried. Returns whether it did.
    pub(super) fn breathe_near_bodies(&mut self, agent: AgentId, position: WorldPosition) -> bool {
        let rotting = self.death_records.iter().any(|record| {
            record
                .position
                .x
                .abs_diff(position.x)
                .max(record.position.y.abs_diff(position.y))
                <= MIASMA_REACH
                && self.body_state(record) == Some(BodyState::Rotting)
        });
        let now = crate::cognition::belief_seconds(self.time);
        let mind = self.minds.get_mut(agent);
        if !rotting || now < mind.sick_until {
            return false;
        }
        mind.sick_until = now.saturating_add(SICK_GAP_SECONDS);
        mind.crafts.saw_burial();
        for need in [NeedKind::Thirst, NeedKind::Rest] {
            let _ =
                self.population
                    .worsen_need(&mut self.scheduler, self.time, agent, need, MIASMA);
        }
        self.sickened += 1;
        true
    }

    /// Buries the body lying at `target` (beside the agent). Anyone watching
    /// sees it done.
    pub(super) fn apply_bury(
        &mut self,
        agent: AgentId,
        target: WorldPosition,
    ) -> Result<(), crate::PolicyFailureReason> {
        let position = self
            .population
            .view(agent)
            .ok_or(crate::PolicyFailureReason::InconsistentState)?
            .position;
        if position
            .x
            .abs_diff(target.x)
            .max(position.y.abs_diff(target.y))
            > 1
        {
            return Err(crate::PolicyFailureReason::TargetUnavailable);
        }
        let (body, _) = self
            .unburied_in(crate::WorldRect {
                min: target,
                max: WorldPosition {
                    x: target.x + 1,
                    y: target.y + 1,
                },
            })
            .into_iter()
            .next()
            .ok_or(crate::PolicyFailureReason::TargetUnavailable)?;
        self.buried.insert(body);
        let watchers: Vec<AgentId> = self
            .perceive_physical(agent, crate::PHYSICAL_POLICY_RADIUS)
            .map(|perception| {
                perception
                    .agents
                    .iter()
                    .filter(|other| {
                        other.id != agent && super::cognition::can_watch(other.activity)
                    })
                    .map(|other| other.id)
                    .collect()
            })
            .unwrap_or_default();
        self.minds.get_mut(agent).crafts.saw_burial();
        for &watcher in &watchers {
            self.minds.get_mut(watcher).crafts.saw_burial();
        }
        self.burial_events.push(BurialEvent {
            agent,
            body,
            at: target,
            watchers: watchers.len() as u16,
        });
        Ok(())
    }

    /// Burials during the latest tick (for logs and tools).
    pub fn burial_events(&self) -> &[BurialEvent] {
        &self.burial_events
    }

    /// People who fell sick by a rotting body, so far.
    pub fn sickened_by_bodies(&self) -> u64 {
        self.sickened
    }
}
