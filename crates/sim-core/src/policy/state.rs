//! Compact per-agent policy state: phase, exploration heading, and decision
//! generation.

use crate::{
    AgentId,
    agent::CompactPosition,
    policy::{ExplorationHeading, PhysicalGoal, PhysicalPolicyView, PolicyReason},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum PolicyPhase {
    Dormant,
    DecisionPending,
    Routing,
    Acting,
    Backoff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub(crate) struct PolicyNavigation(u8);

impl PolicyNavigation {
    const PHASE_MASK: u8 = 0b111;
    const HEADING_SHIFT: u8 = 3;

    const fn new(phase: PolicyPhase, heading: ExplorationHeading) -> Self {
        Self((phase as u8) | ((heading as u8) << Self::HEADING_SHIFT))
    }

    pub(crate) const fn phase(self) -> PolicyPhase {
        match self.0 & Self::PHASE_MASK {
            0 => PolicyPhase::Dormant,
            1 => PolicyPhase::DecisionPending,
            2 => PolicyPhase::Routing,
            3 => PolicyPhase::Acting,
            _ => PolicyPhase::Backoff,
        }
    }

    pub(crate) const fn heading(self) -> ExplorationHeading {
        ExplorationHeading::from_rank(self.0 >> Self::HEADING_SHIFT)
    }

    pub(crate) fn set_phase(&mut self, phase: PolicyPhase) {
        self.0 = (self.0 & !Self::PHASE_MASK) | phase as u8;
    }

    pub(crate) fn set_heading(&mut self, heading: ExplorationHeading) {
        self.0 = (self.0 & Self::PHASE_MASK) | ((heading as u8) << Self::HEADING_SHIFT);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct PolicyState {
    pub(crate) target: CompactPosition,
    pub(crate) generation: u32,
    pub(crate) navigation: PolicyNavigation,
    pub(crate) goal: PhysicalGoal,
    pub(crate) retries: u8,
    pub(crate) reason: PolicyReason,
}

impl Default for PolicyState {
    fn default() -> Self {
        Self {
            target: CompactPosition { x: 0, y: 0 },
            generation: 0,
            navigation: PolicyNavigation::new(PolicyPhase::Dormant, ExplorationHeading::North),
            goal: PhysicalGoal::Wait,
            retries: 0,
            reason: PolicyReason::InitialDecision,
        }
    }
}

impl PolicyState {
    pub(crate) fn for_agent(agent: AgentId) -> Self {
        let mut state = Self::default();
        let mut key = agent.get().wrapping_mul(0x9e37_79b9);
        key ^= key >> 16;
        state
            .navigation
            .set_heading(ExplorationHeading::from_rank(key as u8));
        state
    }

    pub(crate) const fn phase(self) -> PolicyPhase {
        self.navigation.phase()
    }

    pub(crate) fn set_phase(&mut self, phase: PolicyPhase) {
        self.navigation.set_phase(phase);
    }

    pub(crate) const fn exploration_heading(self) -> ExplorationHeading {
        self.navigation.heading()
    }

    pub(crate) fn set_exploration_heading(&mut self, heading: ExplorationHeading) {
        self.navigation.set_heading(heading);
    }

    pub(crate) fn view(self, agent: AgentId) -> PhysicalPolicyView {
        let phase = self.phase();
        PhysicalPolicyView {
            agent,
            goal: self.goal,
            reason: self.reason,
            target: matches!(phase, PolicyPhase::Routing | PolicyPhase::Acting)
                .then(|| self.target.world()),
            committed: matches!(phase, PolicyPhase::Routing | PolicyPhase::Acting),
            retry_count: self.retries,
            exploration_heading: self.exploration_heading(),
        }
    }

    pub(crate) fn next_generation(&mut self) -> Option<u32> {
        self.generation = self.generation.checked_add(1)?;
        Some(self.generation)
    }

    pub(crate) const fn event_is_current(self, generation: u32) -> bool {
        self.generation == generation
            && matches!(
                self.phase(),
                PolicyPhase::DecisionPending | PolicyPhase::Backoff | PolicyPhase::Acting
            )
    }
}
