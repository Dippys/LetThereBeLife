use std::{collections::BTreeMap, error::Error, fmt};

use crate::{BaseResource, Material, World, WorldPosition, WorldQueryError};

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

/// What an agent carries: a count per material, indexed by `Material as usize`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct InventoryView {
    pub items: [u8; Material::COUNT],
}

/// Sparse simulation-owned remaining capacity for one modified generated feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceDeltaView {
    pub position: WorldPosition,
    pub kind: Material,
    pub remaining: u16,
}

impl InventoryView {
    /// An inventory holding `amount` of each listed material.
    pub fn of(contents: &[(Material, u8)]) -> Self {
        let mut inventory = Self::default();
        for &(material, amount) in contents {
            inventory.items[material as usize] = amount;
        }
        inventory
    }

    pub const fn amount(self, kind: Material) -> u8 {
        self.items[kind as usize]
    }

    /// Carried materials with a nonzero count, in material order.
    pub fn carried(self) -> impl Iterator<Item = (Material, u8)> {
        Material::ALL
            .into_iter()
            .map(move |material| (material, self.amount(material)))
            .filter(|&(_, amount)| amount > 0)
    }

    pub const fn remaining_capacity(self, kind: Material) -> u8 {
        INVENTORY_CAPACITY_PER_KIND.saturating_sub(self.amount(kind))
    }

    pub const fn can_add(self, kind: Material) -> bool {
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
    ) -> Result<Option<(Material, u8)>, WorldQueryError> {
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

    /// Marks a generated feature as already used up.
    pub(crate) fn strip(&mut self, position: WorldPosition) {
        let key = CompactFeaturePosition::checked(position)
            .expect("resident world positions fit the finite-world compact envelope");
        self.remaining.insert(key, 0);
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
        assert_eq!(size_of::<InventoryView>(), Material::COUNT);
        assert_eq!(align_of::<InventoryView>(), 1);
        assert_eq!(size_of::<ResourceDelta>(), 6);
        assert_eq!(align_of::<ResourceDelta>(), 2);
    }

    #[test]
    fn inventory_capacity_is_explicit_per_resource_kind() {
        let inventory = InventoryView::of(&[(Material::Berries, 31), (Material::Wood, 32)]);
        assert_eq!(inventory.remaining_capacity(Material::Berries), 1);
        assert!(!inventory.can_add(Material::Wood));
        assert_eq!(inventory.remaining_capacity(Material::Stone), 32);
        assert_eq!(
            InventoryView::of(&[(Material::Berries, u8::MAX)])
                .remaining_capacity(Material::Berries),
            0
        );
    }
}
