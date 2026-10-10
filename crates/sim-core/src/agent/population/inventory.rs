//! Carried inventory, starting supplies, shelter materials, and need relief.

use super::Population;
use crate::{
    NeedKind,
    agent::{ActionEffectError, AgentId, SimTime},
    resources::{FOOD_CONSUMPTION, InitialInventoryError, InventoryView},
    scheduler::Scheduler,
    structures::BuildShelterError,
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
        if inventory
            .items
            .iter()
            .any(|&amount| amount > crate::INVENTORY_CAPACITY_PER_KIND)
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

    pub(crate) fn can_build(&self, agent: AgentId, kind: crate::StructureKind) -> bool {
        self.inventory(agent)
            .is_some_and(|inventory| kind.affordable(inventory))
    }

    pub(crate) fn consume_build_materials(
        &mut self,
        agent: AgentId,
        kind: crate::StructureKind,
    ) -> Result<(), BuildShelterError> {
        let inventory = self
            .inventories
            .get_mut(agent.0 as usize)
            .ok_or(BuildShelterError::MissingAgent)?;
        if !kind.affordable(*inventory) {
            return Err(BuildShelterError::InsufficientMaterials);
        }
        for (material, amount) in kind.cost() {
            inventory.items[material as usize] -= amount;
        }
        Ok(())
    }

    pub(crate) fn refund_build_materials(&mut self, agent: AgentId, kind: crate::StructureKind) {
        let inventory = &mut self.inventories[agent.0 as usize];
        for (material, amount) in kind.cost() {
            let slot = &mut inventory.items[material as usize];
            *slot = slot.saturating_add(amount);
        }
    }

    pub(crate) fn add_inventory(
        &mut self,
        agent: AgentId,
        kind: crate::Material,
        amount: u8,
    ) -> u8 {
        let inventory = &mut self.inventories[agent.0 as usize];
        let accepted = inventory.remaining_capacity(kind).min(amount);
        inventory.items[kind as usize] += accepted;
        accepted
    }

    /// Removes up to `amount` of `material`; returns how much was taken.
    pub(crate) fn take(&mut self, agent: AgentId, material: crate::Material, amount: u8) -> u8 {
        let carried = &mut self.inventories[agent.0 as usize].items[material as usize];
        let taken = (*carried).min(amount);
        *carried -= taken;
        taken
    }

    /// Eats one carried unit of `material`: hunger falls by its nutrition, and
    /// its toxicity is added to thirst and tiredness. Returns its properties
    /// (what the agent feels).
    pub(crate) fn eat(
        &mut self,
        scheduler: &mut Scheduler,
        now: SimTime,
        agent: AgentId,
        material: crate::Material,
    ) -> Result<crate::MaterialProperties, ActionEffectError> {
        let index = agent.0 as usize;
        if self.inventories[index].amount(material) < FOOD_CONSUMPTION {
            return Err(ActionEffectError::NoEdibleInventory);
        }
        if !scheduler.can_schedule(5) {
            return Err(ActionEffectError::EventSequenceExhausted);
        }
        let properties = material.properties();
        let mut next = self.needs[index];
        next.relieve(NeedKind::Hunger, properties.nutrition, now);
        if properties.toxicity > 0 {
            next.worsen(NeedKind::Thirst, properties.toxicity, now);
            next.worsen(NeedKind::Rest, properties.toxicity, now);
        }
        self.schedule_need_thresholds(scheduler, agent, next, now)
            .map_err(|_| ActionEffectError::EventSequenceExhausted)?;
        self.reschedule_health(scheduler, agent, next, now)
            .map_err(|_| ActionEffectError::EventSequenceExhausted)?;
        self.inventories[index].items[material as usize] -= FOOD_CONSUMPTION;
        self.needs[index] = next;
        Ok(properties)
    }

    /// Adds `amount` to one need (a nursing mother gets hungrier).
    pub(crate) fn worsen_need(
        &mut self,
        scheduler: &mut Scheduler,
        now: SimTime,
        agent: AgentId,
        kind: NeedKind,
        amount: u16,
    ) -> Result<(), ActionEffectError> {
        let index = agent.0 as usize;
        if !scheduler.can_schedule(5) {
            return Err(ActionEffectError::EventSequenceExhausted);
        }
        let mut next = self.needs[index];
        next.worsen(kind, amount, now);
        self.schedule_need_thresholds(scheduler, agent, next, now)
            .map_err(|_| ActionEffectError::EventSequenceExhausted)?;
        self.reschedule_health(scheduler, agent, next, now)
            .map_err(|_| ActionEffectError::EventSequenceExhausted)?;
        self.needs[index] = next;
        Ok(())
    }

    pub(crate) fn apply_need_relief(
        &mut self,
        scheduler: &mut Scheduler,
        now: SimTime,
        agent: AgentId,
        kind: NeedKind,
        amount: u16,
    ) -> Result<(), ActionEffectError> {
        let index = agent.0 as usize;
        if !scheduler.can_schedule(5) {
            return Err(ActionEffectError::EventSequenceExhausted);
        }
        let mut next = self.needs[index];
        next.relieve(kind, amount, now);
        self.schedule_need_thresholds(scheduler, agent, next, now)
            .map_err(|_| ActionEffectError::EventSequenceExhausted)?;
        self.reschedule_health(scheduler, agent, next, now)
            .map_err(|_| ActionEffectError::EventSequenceExhausted)?;
        self.needs[index] = next;
        Ok(())
    }
}
