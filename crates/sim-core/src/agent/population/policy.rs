//! Per-agent physical policy state: activation, decision events, commitments,
//! retries, interruption, and timed policy actions.

use super::Population;
use crate::{
    PhysicalNeedsView, WorldPosition,
    agent::{AgentActivity, AgentId, AgentView, CompactPosition, MoveRequestError, SimTime},
    policy::{
        ExplorationHeading, PhysicalGoal, PhysicalPolicyView, PolicyAction, PolicyPhase,
        PolicyReason, PolicyState,
    },
    resources::InventoryView,
    scheduler::{ScheduleError, ScheduledEvent, Scheduler},
    sleep::SleepState,
};

impl Population {
    pub(crate) fn policy_view(&self, agent: AgentId) -> Option<PhysicalPolicyView> {
        self.policies
            .get(agent.0 as usize)
            .copied()
            .map(|state| state.view(agent))
    }

    pub(crate) fn first_non_idle_agent(&self) -> Option<AgentId> {
        self.records
            .iter()
            .position(|record| record.activity != AgentActivity::Idle)
            .map(|index| AgentId(index as u32))
    }

    pub(crate) fn activate_policy(
        &mut self,
        scheduler: &mut Scheduler,
        due: SimTime,
    ) -> Result<(), ScheduleError> {
        for (index, state) in self.policies.iter_mut().enumerate() {
            let generation = state
                .next_generation()
                .ok_or(ScheduleError::SequenceExhausted)?;
            state.set_phase(PolicyPhase::DecisionPending);
            state.goal = PhysicalGoal::Wait;
            state.reason = PolicyReason::InitialDecision;
            scheduler.schedule_decision(
                due,
                AgentId(index as u32),
                generation,
                PhysicalGoal::Wait,
            )?;
        }
        Ok(())
    }

    pub(crate) fn policy_event_is_current(&self, event: ScheduledEvent) -> bool {
        let index = event.agent.0 as usize;
        self.records
            .get(index)
            .is_some_and(|record| !record.activity.is_terminal())
            && self.policies[index].event_is_current(event.generation)
    }

    pub(crate) fn policy_context(
        &self,
        agent: AgentId,
        now: SimTime,
    ) -> Option<(AgentView, PhysicalNeedsView, InventoryView)> {
        let view = self.view(agent)?;
        (!view.activity.is_terminal()).then(|| {
            (
                view,
                self.needs[agent.0 as usize].view(agent, now),
                self.inventories[agent.0 as usize],
            )
        })
    }

    pub(crate) fn commit_policy_route(
        &mut self,
        agent: AgentId,
        goal: PhysicalGoal,
        target: WorldPosition,
        reason: PolicyReason,
        exploration_heading: Option<ExplorationHeading>,
    ) {
        let state = &mut self.policies[agent.0 as usize];
        state.goal = goal;
        state.target =
            CompactPosition::checked(target).expect("policy target is inside active area");
        state.reason = reason;
        if let Some(heading) = exploration_heading {
            state.set_exploration_heading(heading);
        }
        state.set_phase(PolicyPhase::Routing);
        state.retries = 0;
    }

    pub(crate) fn record_policy_retry(
        &mut self,
        agent: AgentId,
        goal: PhysicalGoal,
        target: Option<WorldPosition>,
    ) {
        let Some(state) = self.policies.get_mut(agent.0 as usize) else {
            return;
        };
        state.goal = goal;
        if let Some(target) = target.and_then(CompactPosition::checked) {
            state.target = target;
        }
    }

    pub(crate) fn policy_commitment(
        &self,
        agent: AgentId,
    ) -> Option<(PhysicalGoal, WorldPosition, PolicyReason)> {
        let state = *self.policies.get(agent.0 as usize)?;
        matches!(state.phase(), PolicyPhase::Routing | PolicyPhase::Acting)
            .then(|| (state.goal, state.target.world(), state.reason))
    }

    pub(crate) fn exploration_heading(&self, agent: AgentId) -> Option<ExplorationHeading> {
        self.policies
            .get(agent.0 as usize)
            .copied()
            .map(PolicyState::exploration_heading)
    }

    pub(crate) fn schedule_policy_decision(
        &mut self,
        scheduler: &mut Scheduler,
        now: SimTime,
        agent: AgentId,
        delay: u64,
        reason: PolicyReason,
        retry: bool,
    ) -> Result<SimTime, MoveRequestError> {
        let due = now
            .checked_add(delay)
            .ok_or(MoveRequestError::TimeOverflow)?;
        if !scheduler.can_schedule(1) {
            return Err(MoveRequestError::EventSequenceExhausted);
        }
        let state = self
            .policies
            .get_mut(agent.0 as usize)
            .ok_or(MoveRequestError::MissingAgent)?;
        let generation = state
            .next_generation()
            .ok_or(MoveRequestError::RescheduleLimit)?;
        scheduler
            .schedule_decision(due, agent, generation, state.goal)
            .map_err(|_| MoveRequestError::EventSequenceExhausted)?;
        state.reason = reason;
        let phase = if retry {
            state.retries = state.retries.saturating_add(1);
            PolicyPhase::Backoff
        } else {
            state.retries = 0;
            PolicyPhase::DecisionPending
        };
        state.set_phase(phase);
        Ok(due)
    }

    pub(crate) fn interrupt_for_policy_decision(
        &mut self,
        scheduler: &mut Scheduler,
        now: SimTime,
        agent: AgentId,
        schedule_decision: bool,
    ) -> Result<(SimTime, Option<SleepState>), MoveRequestError> {
        let index = agent.0 as usize;
        let record = self
            .records
            .get(index)
            .ok_or(MoveRequestError::MissingAgent)?;
        if record.activity.is_terminal() {
            return Err(MoveRequestError::DeadAgent);
        }
        let interrupted_sleep = self.sleeps[index].is_active().then_some(self.sleeps[index]);
        let due = if schedule_decision {
            now.checked_add(1).ok_or(MoveRequestError::TimeOverflow)?
        } else {
            now
        };
        let movement_generation = if record.activity == AgentActivity::Moving {
            self.movement_generations[index]
                .checked_add(1)
                .ok_or(MoveRequestError::RescheduleLimit)?
        } else {
            self.movement_generations[index]
        };
        if schedule_decision && self.policies[index].generation == u32::MAX {
            return Err(MoveRequestError::RescheduleLimit);
        }
        let transition_events = if self.needs[index].requires_transition(AgentActivity::Idle) {
            5
        } else {
            0
        };
        if !scheduler.can_schedule(transition_events + u64::from(schedule_decision)) {
            return Err(MoveRequestError::EventSequenceExhausted);
        }
        if record.activity == AgentActivity::Moving {
            self.movement_generations[index] = movement_generation;
            self.routes[index] = None;
        }
        self.transition_activity(scheduler, now, agent, AgentActivity::Idle)
            .expect("event sequence capacity was prechecked");
        self.sleeps[index] = SleepState::default();
        if schedule_decision {
            self.schedule_policy_decision(scheduler, now, agent, 1, PolicyReason::Retry, false)?;
        } else {
            self.policies[index].set_phase(PolicyPhase::Dormant);
        }
        Ok((due, interrupted_sleep))
    }

    pub(crate) fn force_interrupt_sleep(
        &mut self,
        now: SimTime,
        agent: AgentId,
    ) -> Option<SleepState> {
        let index = agent.0 as usize;
        let state = self
            .sleeps
            .get(index)
            .copied()?
            .is_active()
            .then_some(self.sleeps[index])?;
        self.settle_activity_without_events(now, agent, AgentActivity::Idle);
        self.policies[index].set_phase(PolicyPhase::Dormant);
        self.sleeps[index] = SleepState::default();
        Some(state)
    }

    pub(crate) fn force_settle_idle(&mut self, now: SimTime, agent: AgentId) {
        let index = agent.0 as usize;
        if index >= self.records.len() || self.records[index].activity.is_terminal() {
            return;
        }
        self.settle_activity_without_events(now, agent, AgentActivity::Idle);
        self.policies[index].set_phase(PolicyPhase::Dormant);
        self.sleeps[index] = SleepState::default();
    }

    pub(crate) fn schedule_policy_action(
        &mut self,
        scheduler: &mut Scheduler,
        now: SimTime,
        agent: AgentId,
        action: PolicyAction,
    ) -> Result<SimTime, MoveRequestError> {
        let due = now
            .checked_add(action.duration)
            .ok_or(MoveRequestError::TimeOverflow)?;
        let activity = match action.goal {
            PhysicalGoal::Sleep => AgentActivity::Sleeping,
            PhysicalGoal::GatherMaterial => AgentActivity::Gathering,
            PhysicalGoal::BuildShelter => AgentActivity::Building,
            PhysicalGoal::SeekWater
            | PhysicalGoal::SeekFood
            | PhysicalGoal::Drink
            | PhysicalGoal::Eat
            | PhysicalGoal::SeekShelter
            | PhysicalGoal::Explore
            | PhysicalGoal::Signal
            | PhysicalGoal::Hunt
            | PhysicalGoal::Wait => AgentActivity::Idle,
            PhysicalGoal::Incapacitated => AgentActivity::Incapacitated,
        };
        if !scheduler.can_schedule(
            if self.needs[agent.0 as usize].requires_transition(activity) {
                6
            } else {
                1
            },
        ) {
            return Err(MoveRequestError::EventSequenceExhausted);
        }
        self.transition_activity(scheduler, now, agent, activity)?;
        let state = &mut self.policies[agent.0 as usize];
        let generation = state
            .next_generation()
            .ok_or(MoveRequestError::RescheduleLimit)?;
        let compact =
            CompactPosition::checked(action.target).ok_or(MoveRequestError::OutsideWorld)?;
        scheduler
            .schedule_action_completion(due, agent, generation, action.goal, compact)
            .map_err(|_| MoveRequestError::EventSequenceExhausted)?;
        state.goal = action.goal;
        state.target = compact;
        state.reason = action.reason;
        state.set_phase(PolicyPhase::Acting);
        state.retries = 0;
        Ok(due)
    }

    pub(crate) fn complete_policy_action(
        &mut self,
        scheduler: &mut Scheduler,
        event: ScheduledEvent,
    ) -> Result<Option<(PhysicalGoal, WorldPosition, PolicyReason)>, MoveRequestError> {
        if !self.policy_event_is_current(event) {
            return Ok(None);
        }
        let index = event.agent.0 as usize;
        let state = self.policies[index];
        if let Err(error) =
            self.transition_activity(scheduler, event.due, event.agent, AgentActivity::Idle)
        {
            self.settle_activity_without_events(event.due, event.agent, AgentActivity::Idle);
            self.policies[index].set_phase(PolicyPhase::Dormant);
            return Err(error);
        }
        self.policies[index].set_phase(PolicyPhase::Dormant);
        Ok(Some((state.goal, state.target.world(), state.reason)))
    }

    pub(crate) fn policy_retries(&self, agent: AgentId) -> u8 {
        self.policies
            .get(agent.0 as usize)
            .map_or(0, |state| state.retries)
    }
}
