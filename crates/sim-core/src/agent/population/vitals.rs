//! Needs and health: threshold and consequence events, incapacitation, death,
//! and test-only state setters.

use super::Population;
use crate::{
    NeedKind, NeedQueryError, NeedThresholdEventOutcome, NeedThresholdOutcomeKind,
    PhysicalNeedsView,
    agent::{AgentActivity, AgentId, EventId, SimTime},
    health::{DeathCause, DeathRecord, HealthDiagnostic, HealthDiagnosticKind, HealthView},
    needs::NeedState,
    policy::PolicyPhase,
    scheduler::{ScheduleError, ScheduledEvent, Scheduler},
    sleep::SleepState,
};

impl Population {
    pub(crate) fn initialize_need_events(
        &mut self,
        scheduler: &mut Scheduler,
        now: SimTime,
    ) -> Result<(), ScheduleError> {
        for index in 0..self.needs.len() {
            let agent = AgentId(index as u32);
            let state = self.needs[index];
            self.schedule_need_thresholds(scheduler, agent, state, now)?;
            self.reschedule_health(scheduler, agent, state, now)?;
        }
        Ok(())
    }

    pub(crate) fn apply_health_consequence(
        &mut self,
        scheduler: &mut Scheduler,
        event: ScheduledEvent,
    ) -> HealthDiagnostic {
        let index = event.agent.0 as usize;
        let Some(health) = self.health.get_mut(index) else {
            return HealthDiagnostic {
                agent: event.agent,
                at: event.due,
                cause: None,
                before: 0,
                after: 0,
                kind: HealthDiagnosticKind::StaleEvent,
            };
        };
        let outcome = health.apply(event.generation, self.needs[index], event.due, event.agent);
        if outcome.kind != HealthDiagnosticKind::Died
            && outcome.kind != HealthDiagnosticKind::StaleEvent
            && let Some(due) = health.schedule_next_interval(event.due)
        {
            let _ = scheduler.schedule_health_consequence(
                due,
                event.agent,
                health.generation(),
                event.need,
            );
        }
        outcome
    }

    /// Wounds a living agent at once (a bite). `None` if it isn't alive.
    pub(crate) fn injure(
        &mut self,
        agent: AgentId,
        amount: u16,
        now: SimTime,
    ) -> Option<HealthDiagnostic> {
        let index = agent.0 as usize;
        if self
            .records
            .get(index)
            .is_none_or(|record| record.activity == AgentActivity::Dead)
        {
            return None;
        }
        Some(self.health[index].injure(amount, agent, now))
    }

    pub(crate) fn health_view(&self, agent: AgentId) -> Option<HealthView> {
        self.health
            .get(agent.0 as usize)
            .copied()
            .map(|state| state.view(agent))
    }

    pub(crate) fn living_count(&self) -> usize {
        self.living_count as usize
    }

    pub(crate) fn active_count(&self) -> usize {
        self.active_count as usize
    }

    pub(crate) fn incapacitate(&mut self, now: SimTime, agent: AgentId) {
        let index = agent.0 as usize;
        if !self
            .records
            .get(index)
            .is_some_and(|record| record.activity != AgentActivity::Dead)
        {
            return;
        }
        if self.records[index].activity == AgentActivity::Incapacitated {
            return;
        }
        self.routes[index] = None;
        self.movement_generations[index] = self.movement_generations[index].wrapping_add(1);
        self.policies[index].set_phase(PolicyPhase::Dormant);
        self.policies[index].generation = self.policies[index].generation.wrapping_add(1);
        self.sleeps[index] = SleepState::default();
        self.active_count = self.active_count.saturating_sub(1);
        self.settle_activity_without_events(now, agent, AgentActivity::Incapacitated);
    }

    pub(crate) fn finalize_death(
        &mut self,
        at: SimTime,
        agent: AgentId,
        cause: DeathCause,
    ) -> Option<DeathRecord> {
        let index = agent.0 as usize;
        let record = self.records.get(index)?;
        if record.activity == AgentActivity::Dead {
            return None;
        }
        let position = record.position.world();
        let was_active = !record.activity.is_terminal();
        self.routes[index] = None;
        self.movement_generations[index] = self.movement_generations[index].wrapping_add(1);
        self.policies[index].set_phase(PolicyPhase::Dormant);
        self.policies[index].generation = self.policies[index].generation.wrapping_add(1);
        self.sleeps[index] = SleepState::default();
        self.spatial.remove(agent, position);
        self.living_count = self.living_count.saturating_sub(1);
        if was_active {
            self.active_count = self.active_count.saturating_sub(1);
        }
        self.settle_activity_without_events(at, agent, AgentActivity::Dead);
        Some(DeathRecord {
            agent,
            cause,
            at,
            position,
        })
    }

    pub(crate) fn needs_view(
        &self,
        agent: AgentId,
        now: SimTime,
    ) -> Result<PhysicalNeedsView, NeedQueryError> {
        let record = self
            .records
            .get(agent.0 as usize)
            .ok_or(NeedQueryError::MissingAgent)?;
        if record.activity.is_terminal() {
            return Err(NeedQueryError::DeadAgent);
        }
        Ok(self.needs[agent.0 as usize].view(agent, now))
    }

    pub(crate) fn apply_need_threshold(
        &mut self,
        event: ScheduledEvent,
    ) -> NeedThresholdEventOutcome {
        let index = event.agent.0 as usize;
        let Some(record) = self.records.get(index) else {
            return need_outcome(event, None, NeedThresholdOutcomeKind::MissingAgent);
        };
        if record.activity.is_terminal() {
            return need_outcome(event, None, NeedThresholdOutcomeKind::DeadAgent);
        }
        let (outcome, value) =
            self.needs[index].apply_threshold(event.generation, event.need, event.due);
        need_outcome(event, Some(value), outcome)
    }

    pub(super) fn schedule_need_thresholds(
        &self,
        scheduler: &mut Scheduler,
        agent: AgentId,
        state: NeedState,
        now: SimTime,
    ) -> Result<(), ScheduleError> {
        for kind in NeedKind::ALL {
            if let Some(due) = state.threshold_due(kind, now) {
                scheduler.schedule_need_threshold(due, agent, state.generation(), kind)?;
            }
        }
        Ok(())
    }

    pub(super) fn reschedule_health(
        &mut self,
        scheduler: &mut Scheduler,
        agent: AgentId,
        needs: NeedState,
        now: SimTime,
    ) -> Result<(), ScheduleError> {
        let health = &mut self.health[agent.0 as usize];
        if let Some((due, need)) = health.reschedule(needs, now) {
            scheduler.schedule_health_consequence(due, agent, health.generation(), need)?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn set_need_value_for_test(
        &mut self,
        agent: AgentId,
        kind: NeedKind,
        value: u16,
        now: SimTime,
    ) {
        self.needs[agent.0 as usize].set_value_for_test(kind, value, now);
    }

    #[cfg(test)]
    pub(crate) fn prepare_health_consequence_for_test(
        &mut self,
        scheduler: &mut Scheduler,
        agent: AgentId,
        value: u16,
        now: SimTime,
    ) {
        let index = agent.0 as usize;
        self.health[index].set_value_for_test(value);
        let needs = self.needs[index];
        self.reschedule_health(scheduler, agent, needs, now)
            .unwrap();
    }

    #[cfg(test)]
    pub(crate) fn mark_dead(&mut self, agent: AgentId) {
        self.records[agent.0 as usize].activity = AgentActivity::Dead;
    }
}

fn need_outcome(
    event: ScheduledEvent,
    value: Option<u16>,
    outcome: NeedThresholdOutcomeKind,
) -> NeedThresholdEventOutcome {
    NeedThresholdEventOutcome {
        event: EventId(event.sequence),
        agent: event.agent,
        due: event.due,
        kind: event.need,
        value,
        outcome,
    }
}
