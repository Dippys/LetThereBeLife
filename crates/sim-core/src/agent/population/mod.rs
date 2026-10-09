//! The dense population store and its shared accessors. Behaviour is split
//! across submodules by responsibility; each adds an `impl Population` block.

mod init;
mod inventory;
mod movement;
mod perception;
mod policy;
mod sleep;
mod vitals;

use crate::{
    World, WorldRect,
    agent::{
        AgentActivity, AgentId, AgentRecord, AgentView, CompactPosition, MoveRequestError, SimTime,
    },
    health::HealthState,
    needs::NeedState,
    placements::SpawnedObjects,
    policy::PolicyState,
    resources::InventoryView,
    scheduler::{EventClass, ScheduledEvent, Scheduler},
    sleep::SleepState,
    spatial::SpatialIndex,
    structures::StructureStore,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct RouteState {
    destination: CompactPosition,
    max_expansions: u16,
}

#[derive(Clone, Copy)]
pub(crate) struct MovementEnvironment<'a> {
    pub(crate) world: &'a World,
    pub(crate) spawned_objects: &'a SpawnedObjects,
    pub(crate) structures: &'a StructureStore,
}

#[derive(Debug, Default)]
pub(crate) struct Population {
    records: Vec<AgentRecord>,
    movement_generations: Vec<u32>,
    routes: Vec<Option<RouteState>>,
    needs: Vec<NeedState>,
    policies: Vec<PolicyState>,
    inventories: Vec<InventoryView>,
    sleeps: Vec<SleepState>,
    health: Vec<HealthState>,
    spatial: SpatialIndex,
    living_count: u32,
    active_count: u32,
    active_area: Option<WorldRect>,
    initialized: bool,
}

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct PopulationCapacities {
    pub(crate) records: usize,
    pub(crate) movement_generations: usize,
    pub(crate) routes: usize,
    pub(crate) needs: usize,
    pub(crate) policies: usize,
    pub(crate) inventories: usize,
    pub(crate) sleeps: usize,
    pub(crate) health: usize,
    pub(crate) occupancy_entries: usize,
    pub(crate) occupancy_entry_capacity: usize,
    pub(crate) occupancy_buckets: usize,
}

impl Population {
    fn transition_activity(
        &mut self,
        scheduler: &mut Scheduler,
        now: SimTime,
        agent: AgentId,
        activity: AgentActivity,
    ) -> Result<(), MoveRequestError> {
        let index = agent.0 as usize;
        if self.needs[index].requires_transition(activity) && !scheduler.can_schedule(5) {
            return Err(MoveRequestError::EventSequenceExhausted);
        }
        if self.needs[index].transition(activity, now) {
            let state = self.needs[index];
            self.schedule_need_thresholds(scheduler, agent, state, now)
                .map_err(|_| MoveRequestError::EventSequenceExhausted)?;
            self.reschedule_health(scheduler, agent, state, now)
                .map_err(|_| MoveRequestError::EventSequenceExhausted)?;
        }
        self.records[index].activity = activity;
        Ok(())
    }

    fn settle_activity_without_events(
        &mut self,
        now: SimTime,
        agent: AgentId,
        activity: AgentActivity,
    ) {
        let index = agent.0 as usize;
        self.needs[index].transition(activity, now);
        self.records[index].activity = activity;
    }

    pub(crate) fn views(&self, limit: usize) -> impl Iterator<Item = AgentView> + '_ {
        self.records
            .iter()
            .take(limit)
            .enumerate()
            .map(|(index, record)| AgentView {
                id: AgentId(index as u32),
                position: record.position.world(),
                activity: record.activity,
            })
    }

    pub(crate) fn view(&self, agent: AgentId) -> Option<AgentView> {
        let record = self.records.get(agent.0 as usize)?;
        Some(AgentView {
            id: agent,
            position: record.position.world(),
            activity: record.activity,
        })
    }

    pub(crate) fn spatial(&self) -> &SpatialIndex {
        &self.spatial
    }

    pub(crate) fn active_area(&self) -> Option<WorldRect> {
        self.active_area
    }

    pub(crate) fn len(&self) -> usize {
        self.records.len()
    }

    pub(crate) fn is_initialized(&self) -> bool {
        self.initialized
    }

    pub(crate) fn event_is_current(&self, event: &ScheduledEvent) -> bool {
        let index = event.agent.0 as usize;
        self.records
            .get(index)
            .is_some_and(|record| match event.class {
                EventClass::Movement => {
                    record.activity == AgentActivity::Moving
                        && self.movement_generations[index] == event.generation
                }
                EventClass::NeedThreshold => {
                    !record.activity.is_terminal()
                        && self.needs[index].event_is_current(event.generation, event.need)
                }
                EventClass::HealthConsequence => {
                    record.activity != AgentActivity::Dead
                        && self.health[index].event_is_current(event.generation)
                }
                EventClass::Decision | EventClass::Wake | EventClass::ActionCompletion => {
                    !record.activity.is_terminal()
                        && self.policies[index].event_is_current(event.generation)
                }
            })
    }

    pub(crate) fn capacities(&self) -> PopulationCapacities {
        PopulationCapacities {
            records: self.records.capacity(),
            movement_generations: self.movement_generations.capacity(),
            routes: self.routes.capacity(),
            needs: self.needs.capacity(),
            policies: self.policies.capacity(),
            inventories: self.inventories.capacity(),
            sleeps: self.sleeps.capacity(),
            health: self.health.capacity(),
            occupancy_entries: self.spatial.len(),
            occupancy_entry_capacity: self.spatial.retained_entry_capacity(),
            occupancy_buckets: self.spatial.bucket_count(),
        }
    }

    #[cfg(test)]
    pub(crate) fn inventory_capacity(&self) -> usize {
        self.inventories.capacity()
    }

    #[cfg(test)]
    pub(crate) fn sleep_capacity(&self) -> usize {
        self.sleeps.capacity()
    }
}
