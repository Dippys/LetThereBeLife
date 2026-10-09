//! Carried inventory, starting supplies, shelter materials, and need relief.

use super::Population;
use crate::{
    NeedKind,
    agent::{ActionEffectError, AgentId, SimTime},
    resources::{FOOD_CONSUMPTION, InitialInventoryError, InventoryView},
    scheduler::Scheduler,
    structures::{BuildShelterError, SHELTER_WOOD_COST},
};

impl Population {
    pub(crate) fn inventory(&self, agent: AgentId) -> Option<InventoryView> {
        let index = agent.0 as usize;
        self.records
            .get(index)
            .is_some_and(|record| !record.activity.is_terminal())
            .then(|| self.inventories[index])
    }

    pub(crate) fn set_initial_inventory(
        &mut self,
        agent: AgentId,
        inventory: InventoryView,
    ) -> Result<(), InitialInventoryError> {
        if inventory.food > crate::INVENTORY_CAPACITY_PER_KIND
            || inventory.wood > crate::INVENTORY_CAPACITY_PER_KIND
            || inventory.stone > crate::INVENTORY_CAPACITY_PER_KIND
        {
            return Err(InitialInventoryError::AmountExceedsCapacity);
        }
        let slot = self
            .inventories
            .get_mut(agent.0 as usize)
            .ok_or(InitialInventoryError::MissingAgent)?;
        *slot = inventory;
        Ok(())
    }

    pub(crate) fn can_build_shelter(&self, agent: AgentId) -> bool {
        self.inventory(agent)
            .is_some_and(|inventory| inventory.wood >= SHELTER_WOOD_COST)
    }

    pub(crate) fn consume_shelter_materials(
        &mut self,
        agent: AgentId,
    ) -> Result<(), BuildShelterError> {
        let inventory = self
            .inventories
            .get_mut(agent.0 as usize)
            .ok_or(BuildShelterError::MissingAgent)?;
        if inventory.wood < SHELTER_WOOD_COST {
            return Err(BuildShelterError::InsufficientMaterials);
        }
        inventory.wood -= SHELTER_WOOD_COST;
        Ok(())
    }

    pub(crate) fn refund_shelter_materials(&mut self, agent: AgentId) {
        let inventory = &mut self.inventories[agent.0 as usize];
        inventory.wood = inventory.wood.saturating_add(SHELTER_WOOD_COST);
    }

    pub(crate) fn add_inventory(
        &mut self,
        agent: AgentId,
        kind: crate::ResourceKind,
        amount: u8,
    ) -> u8 {
        let inventory = &mut self.inventories[agent.0 as usize];
        let accepted = inventory.remaining_capacity(kind).min(amount);
        let slot = match kind {
            crate::ResourceKind::Food => &mut inventory.food,
            crate::ResourceKind::Wood => &mut inventory.wood,
            crate::ResourceKind::Stone => &mut inventory.stone,
        };
        *slot += accepted;
        accepted
    }

    pub(crate) fn apply_need_relief(
        &mut self,
        scheduler: &mut Scheduler,
        now: SimTime,
        agent: AgentId,
        kind: NeedKind,
        amount: u16,
        consume_food: bool,
    ) -> Result<(), ActionEffectError> {
        let index = agent.0 as usize;
        if consume_food && self.inventories[index].food < FOOD_CONSUMPTION {
            return Err(ActionEffectError::NoEdibleInventory);
        }
        if !scheduler.can_schedule(5) {
            return Err(ActionEffectError::EventSequenceExhausted);
        }
        let mut next = self.needs[index];
        next.relieve(kind, amount, now);
        self.schedule_need_thresholds(scheduler, agent, next, now)
            .map_err(|_| ActionEffectError::EventSequenceExhausted)?;
        self.reschedule_health(scheduler, agent, next, now)
            .map_err(|_| ActionEffectError::EventSequenceExhausted)?;
        if consume_food {
            self.inventories[index].food -= FOOD_CONSUMPTION;
        }
        self.needs[index] = next;
        Ok(())
    }
}
