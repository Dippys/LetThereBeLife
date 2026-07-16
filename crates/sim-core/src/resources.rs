use std::{collections::BTreeMap, error::Error, fmt};

use crate::{BaseResource, ResourceKind, World, WorldPosition, WorldQueryError};

pub const INVENTORY_CAPACITY_PER_KIND: u8 = 32;
pub const GATHER_YIELD: u8 = 4;
pub const FOOD_CONSUMPTION: u8 = 1;
pub const EAT_HUNGER_RELIEF: u16 = 4_000;
pub const DRINK_THIRST_RELIEF: u16 = 5_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitialInventoryError {
    PolicyActive,
    SimulationAdvanced,
    MissingAgent,
    AmountExceedsCapacity,
}

impl fmt::Display for InitialInventoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "initial inventory setup failed: {self:?}")
    }
}

impl Error for InitialInventoryError {}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct InventoryView {
    pub food: u8,
    pub wood: u8,
    pub stone: u8,
}

/// Sparse simulation-owned remaining capacity for one modified generated feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceDeltaView {
    pub position: WorldPosition,
    pub kind: ResourceKind,
    pub remaining: u16,
}

impl InventoryView {
    pub const fn amount(self, kind: ResourceKind) -> u8 {
        match kind {
            ResourceKind::Food => self.food,
            ResourceKind::Wood => self.wood,
            ResourceKind::Stone => self.stone,
        }
    }

    pub const fn remaining_capacity(self, kind: ResourceKind) -> u8 {
        INVENTORY_CAPACITY_PER_KIND.saturating_sub(self.amount(kind))
    }

    pub const fn can_add(self, kind: ResourceKind) -> bool {
        self.remaining_capacity(kind) > 0
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct ResourceDelta {
    position: CompactFeaturePosition,
    remaining: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(C)]
struct CompactFeaturePosition {
    x: i16,
    y: i16,
}

impl CompactFeaturePosition {
    fn checked(position: WorldPosition) -> Option<Self> {
        Some(Self {
            x: i16::try_from(position.x).ok()?,
            y: i16::try_from(position.y).ok()?,
        })
    }
}

#[derive(Debug, Default)]
pub(crate) struct ResourceDeltas {
    remaining: BTreeMap<CompactFeaturePosition, u16>,
}

impl ResourceDeltas {
    pub(crate) fn resource_at(
        &self,
        world: &World,
        position: WorldPosition,
    ) -> Result<Option<BaseResource>, WorldQueryError> {
        let Some(base) = world.resource_at(position)? else {
            return Ok(None);
        };
        let key = CompactFeaturePosition::checked(position)
            .expect("resident world positions fit the finite-world compact envelope");
        let remaining = self.remaining.get(&key).copied().unwrap_or(base.capacity);
        Ok((remaining > 0).then_some(BaseResource {
            capacity: remaining,
            kind: base.kind,
        }))
    }

    pub(crate) fn gather(
        &mut self,
        world: &World,
        position: WorldPosition,
        maximum: u8,
    ) -> Result<Option<(ResourceKind, u8)>, WorldQueryError> {
        let Some(current) = self.resource_at(world, position)? else {
            return Ok(None);
        };
        let gathered = current.capacity.min(u16::from(maximum)) as u8;
        if gathered == 0 {
            return Ok(None);
        }
        let key = CompactFeaturePosition::checked(position)
            .expect("resident world positions fit the finite-world compact envelope");
        self.remaining
            .insert(key, current.capacity - u16::from(gathered));
        Ok(Some((current.kind, gathered)))
    }

    pub(crate) fn len(&self) -> usize {
        self.remaining.len()
    }

    pub(crate) fn views<'a>(
        &'a self,
        world: &'a World,
    ) -> impl Iterator<Item = ResourceDeltaView> + 'a {
        self.remaining.iter().map(|(position, &remaining)| {
            let position = WorldPosition {
                x: i64::from(position.x),
                y: i64::from(position.y),
            };
            let resource = world
                .base_resource_at(position)
                .expect("resource delta keys remain backed by generated features");
            ResourceDeltaView {
                position,
                kind: resource.kind,
                remaining,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::mem::{align_of, size_of};

    use super::*;

    #[test]
    fn compact_inventory_and_delta_layouts_are_fixed() {
        assert_eq!(size_of::<InventoryView>(), 3);
        assert_eq!(align_of::<InventoryView>(), 1);
        assert_eq!(size_of::<ResourceDelta>(), 6);
        assert_eq!(align_of::<ResourceDelta>(), 2);
    }

    #[test]
    fn inventory_capacity_is_explicit_per_resource_kind() {
        let inventory = InventoryView {
            food: 31,
            wood: 32,
            stone: 0,
        };
        assert_eq!(inventory.remaining_capacity(ResourceKind::Food), 1);
        assert!(!inventory.can_add(ResourceKind::Wood));
        assert_eq!(inventory.remaining_capacity(ResourceKind::Stone), 32);
        assert_eq!(
            InventoryView {
                food: u8::MAX,
                ..InventoryView::default()
            }
            .remaining_capacity(ResourceKind::Food),
            0
        );
    }
}
