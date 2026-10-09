//! Shelter construction: build requests, start, completion, and cancellation
//! with material refunds.

use super::errors::map_build_schedule_error;
use crate::policy::PolicyAction;
use crate::{
    AgentActivity, AgentId, BuildShelterError, Engine, PhysicalGoal, PolicyFailureReason,
    PolicyReason, SHELTER_BUILD_TICKS, SHELTER_STONE_COST, SHELTER_WOOD_COST, Standability,
    StructureDiagnostic, StructureDiagnosticKind, StructureView, WORLD_GENERATION_BOUNDS,
    WorldPosition, WorldQueryError,
};

impl Engine {
    /// Starts a one-cell shelter build from a cardinally adjacent access cell.
    pub fn request_build_shelter(
        &mut self,
        agent: AgentId,
        site: WorldPosition,
    ) -> Result<StructureView, BuildShelterError> {
        if self.policy_active {
            return Err(BuildShelterError::PolicyControlled);
        }
        self.start_shelter_build(agent, site, PolicyReason::NoUrgentNeed)
    }

    pub(super) fn start_shelter_build(
        &mut self,
        agent: AgentId,
        site: WorldPosition,
        reason: PolicyReason,
    ) -> Result<StructureView, BuildShelterError> {
        let view = self
            .population
            .view(agent)
            .ok_or(BuildShelterError::MissingAgent)?;
        if view.activity == AgentActivity::Dead {
            return Err(BuildShelterError::DeadAgent);
        }
        if view.activity != AgentActivity::Idle {
            return Err(BuildShelterError::AgentCommitted);
        }
        if !WORLD_GENERATION_BOUNDS.contains(site) {
            return Err(BuildShelterError::OutsideWorld);
        }
        if !self
            .population
            .active_area()
            .is_some_and(|area| area.contains(site))
        {
            return Err(BuildShelterError::OutsideActiveArea);
        }
        if view.position.x.abs_diff(site.x) + view.position.y.abs_diff(site.y) != 1 {
            return Err(BuildShelterError::NotCardinallyAdjacent);
        }
        self.structures.can_start(agent, site)?;
        if let Some(occupant) = self.population.spatial().occupant(site) {
            return Err(BuildShelterError::Occupied(occupant));
        }
        match self.spawned_objects.standability_at(&self.world, site) {
            Ok(Standability::Standable) => {}
            Ok(Standability::BlockedByWater) => return Err(BuildShelterError::Water),
            Ok(Standability::BlockedByFeature) => {
                return Err(BuildShelterError::BlockingFeature);
            }
            Err(WorldQueryError::Unloaded) => return Err(BuildShelterError::Unloaded),
            Err(WorldQueryError::OutsideWorldBounds) => {
                return Err(BuildShelterError::OutsideWorld);
            }
            Err(WorldQueryError::NonCardinalStep) => unreachable!("standing queries have no step"),
        }
        if self
            .spawned_objects
            .reserves_exclusive_use_at(&self.world, site)
        {
            return Err(BuildShelterError::BlockingFeature);
        }
        if !self.population.can_build_shelter(agent) {
            return Err(BuildShelterError::InsufficientMaterials);
        }
        self.compact_scheduler_if_needed();
        let due = self
            .population
            .schedule_policy_action(
                &mut self.scheduler,
                self.time,
                agent,
                PolicyAction {
                    goal: PhysicalGoal::BuildShelter,
                    target: site,
                    reason,
                    duration: SHELTER_BUILD_TICKS,
                },
            )
            .map_err(map_build_schedule_error)?;
        let structure = self
            .structures
            .start(agent, site, self.time, due)
            .expect("construction capacity and conflicts were prevalidated");
        self.population
            .consume_shelter_materials(agent)
            .expect("shelter recipe was prevalidated");
        self.structure_diagnostics.push(StructureDiagnostic {
            structure,
            at: self.time,
            kind: StructureDiagnosticKind::Started,
            refunded_wood: 0,
            refunded_stone: 0,
        });
        Ok(structure)
    }

    pub(super) fn apply_build_completion(
        &mut self,
        agent: AgentId,
    ) -> Result<(), PolicyFailureReason> {
        let structure = self
            .structures
            .complete_for_builder(agent)
            .ok_or(PolicyFailureReason::InconsistentState)?;
        self.structure_diagnostics.push(StructureDiagnostic {
            structure,
            at: self.time,
            kind: StructureDiagnosticKind::Completed,
            refunded_wood: 0,
            refunded_stone: 0,
        });
        Ok(())
    }

    pub(super) fn cancel_construction(&mut self, agent: AgentId) -> bool {
        let Some(structure) = self.structures.cancel_for_builder(agent) else {
            return false;
        };
        self.population.refund_shelter_materials(agent);
        self.structure_diagnostics.push(StructureDiagnostic {
            structure,
            at: self.time,
            kind: StructureDiagnosticKind::Cancelled,
            refunded_wood: SHELTER_WOOD_COST,
            refunded_stone: SHELTER_STONE_COST,
        });
        true
    }
}
