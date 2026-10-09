//! Sleep location validation, sleep scheduling, and sleep state queries.

use super::{MovementEnvironment, Population};
use crate::{
    Standability, WorldPosition, WorldQueryError,
    agent::{AgentActivity, AgentId, CompactPosition, SimTime},
    policy::{PhysicalGoal, PolicyPhase, PolicyReason},
    scheduler::Scheduler,
    sleep::{SleepQuality, SleepRequestError, SleepState, SleepView},
    structures::StructureId,
};

impl Population {
    pub(crate) fn validate_sleep_location(
        &self,
        environment: MovementEnvironment<'_>,
        now: SimTime,
        agent: AgentId,
        position: WorldPosition,
        sheltered: bool,
        structure: Option<StructureId>,
    ) -> Result<SleepQuality, SleepRequestError> {
        let index = agent.0 as usize;
        let record = self
            .records
            .get(index)
            .ok_or(SleepRequestError::MissingAgent)?;
        if record.activity.is_terminal() {
            return Err(SleepRequestError::DeadAgent);
        }
        if record.activity != AgentActivity::Idle {
            return Err(SleepRequestError::AgentCommitted);
        }
        if !self
            .active_area
            .expect("initialized population")
            .contains(position)
        {
            return Err(SleepRequestError::OutsideActiveArea);
        }
        match environment
            .spawned_objects
            .standability_at(environment.world, position)
        {
            Ok(Standability::Standable) => {}
            Ok(Standability::BlockedByWater) => return Err(SleepRequestError::Water),
            Ok(Standability::BlockedByFeature) => {
                return Err(SleepRequestError::BlockingFeature);
            }
            Err(WorldQueryError::Unloaded) => return Err(SleepRequestError::Unloaded),
            Err(WorldQueryError::OutsideWorldBounds) => {
                return Err(SleepRequestError::OutsideWorld);
            }
            Err(WorldQueryError::NonCardinalStep) => unreachable!("standing queries have no step"),
        }
        if environment
            .spawned_objects
            .reserves_exclusive_use_at(environment.world, position)
        {
            return Err(SleepRequestError::BlockingFeature);
        }
        if let Some(occupant) = self.spatial.occupant_except(position, agent) {
            return Err(SleepRequestError::Occupied(occupant));
        }
        if let Some(structure) = structure {
            return Err(SleepRequestError::StructureOccupied(structure));
        }
        if !sheltered
            && self.needs[index]
                .view(agent, now)
                .exposure
                .threshold_reached
        {
            return Err(SleepRequestError::UnsafeExposure);
        }
        if record.position.world() != position {
            return Err(SleepRequestError::NotAtLocation);
        }
        Ok(if sheltered {
            SleepQuality::Sheltered
        } else {
            SleepQuality::OpenGround
        })
    }

    pub(crate) fn schedule_sleep(
        &mut self,
        scheduler: &mut Scheduler,
        now: SimTime,
        agent: AgentId,
        position: WorldPosition,
        quality: SleepQuality,
        reason: PolicyReason,
    ) -> Result<SleepView, SleepRequestError> {
        let index = agent.0 as usize;
        let compact = CompactPosition::checked(position).ok_or(SleepRequestError::OutsideWorld)?;
        let due = self.needs[index]
            .sleep_recovery_due_for(quality, now, reason == PolicyReason::ExposureThreshold)
            .ok_or(SleepRequestError::TimeOverflow)?;
        if self.policies[index].generation == u32::MAX {
            return Err(SleepRequestError::RescheduleLimit);
        }
        let transition_events = if self.needs[index].requires_transition(AgentActivity::Sleeping) {
            5
        } else {
            0
        };
        if !scheduler.can_schedule(transition_events + 1) {
            return Err(SleepRequestError::EventSequenceExhausted);
        }
        if self.needs[index].transition_sleep(quality, now) {
            let state = self.needs[index];
            self.schedule_need_thresholds(scheduler, agent, state, now)
                .expect("event sequence capacity was prechecked");
            self.reschedule_health(scheduler, agent, state, now)
                .expect("event sequence capacity was prechecked");
        }
        self.records[index].activity = AgentActivity::Sleeping;
        let state = &mut self.policies[index];
        let generation = state
            .next_generation()
            .expect("policy generation was prechecked");
        scheduler
            .schedule_wake(due, agent, generation, compact)
            .expect("event sequence capacity was prechecked");
        state.goal = PhysicalGoal::Sleep;
        state.target = compact;
        state.reason = reason;
        state.set_phase(PolicyPhase::Acting);
        state.retries = 0;
        self.sleeps[index] = SleepState::active(now, due, quality);
        Ok(self.sleep_view(agent).expect("sleep was just activated"))
    }

    pub(crate) fn sleep_view(&self, agent: AgentId) -> Option<SleepView> {
        let index = agent.0 as usize;
        let state = *self.sleeps.get(index)?;
        state.is_active().then(|| SleepView {
            agent,
            position: self.records[index].position.world(),
            started_at: state.started_at,
            planned_wake: state.planned_wake,
            quality: state.quality,
        })
    }

    pub(crate) fn finish_sleep(&mut self, agent: AgentId) -> Option<SleepView> {
        let view = self.sleep_view(agent)?;
        self.sleeps[agent.0 as usize] = SleepState::default();
        Some(view)
    }
}
