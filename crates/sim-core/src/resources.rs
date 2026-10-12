use std::{collections::BTreeMap, error::Error, fmt};

use crate::{BaseResource, Material, World, WorldPosition, WorldQueryError};

/// How many things one person can carry in all (any mix of materials).
pub const CARRY_CAPACITY: u8 = 12;
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
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

    /// Everything carried, of every material.
    pub const fn total(self) -> u8 {
        let mut total = 0_u8;
        let mut index = 0;
        while index < Material::COUNT {
            total = total.saturating_add(self.items[index]);
            index += 1;
        }
        total
    }

    /// How much it can carry in all: two hands' worth, more with a basket.
    pub fn capacity(self) -> u8 {
        CARRY_CAPACITY
            + self
                .carried()
                .map(|(material, _)| material.properties().room)
                .max()
                .unwrap_or(0)
    }

    /// Room left for more of `kind`: the load is shared by every material.
    pub fn remaining_capacity(self, _kind: Material) -> u8 {
        self.capacity().saturating_sub(self.total())
    }

    /// A night's rot: of everything that goes bad, about one part in its
    /// keeping days (times `slower`) is lost. Returns how many units rotted.
    pub fn rot(&mut self, slower: u8) -> u32 {
        let mut lost = 0;
        for material in Material::ALL {
            let keeps = material.properties().keeps_days;
            let amount = &mut self.items[material as usize];
            if keeps > 0 && *amount > 0 {
                let rotten = amount.div_ceil(keeps.saturating_mul(slower.max(1)));
                *amount -= rotten;
                lost += u32::from(rotten);
            }
        }
        lost
    }

    pub fn can_add(self, kind: Material) -> bool {
        self.remaining_capacity(kind) > 0
    }
}

/// The layout of one stored depletion entry (key and value), for size checks.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct ResourceDelta {
    position: CompactFeaturePosition,
    remaining: u16,
    since: u32,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
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

/// What's left of one modified feature, and when that was last true.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Delta {
    remaining: u16,
    /// Simulated second it was last gathered (regrowth counts from here).
    since: u32,
}

/// Sparse depletion of generated features. Sources grow back lazily: what's
/// left is computed from the last gathering and the material's regrowth time
/// whenever someone looks, so nothing scans every bush every tick.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct ResourceDeltas {
    remaining: BTreeMap<CompactFeaturePosition, Delta>,
    /// The engine's current time in seconds, advanced every tick.
    now: u32,
    /// Scenario switch: nothing grows back (a famine valley).
    barren: bool,
}

/// `remaining` (last true at second `since`) grown back to `now`, capped at
/// the generated capacity. A dead plant grows nothing.
pub(crate) fn regrown(remaining: u16, since: u32, base: BaseResource, now: u32) -> u16 {
    let period = base.kind.properties().regrow_seconds;
    if period == 0 || since == DEAD {
        return remaining;
    }
    let properties = base.kind.properties();
    let elapsed = if properties.seasonal {
        crate::season::growing_seconds(since, now)
    } else {
        now.saturating_sub(since)
    };
    let grown = elapsed / period;
    let grown = u16::try_from(grown).unwrap_or(u16::MAX);
    remaining.saturating_add(grown).min(base.capacity)
}

/// `Delta::since` of a plant that died or was felled.
const DEAD: u32 = u32::MAX;

impl ResourceDeltas {
    /// Moves the clock regrowth is measured against. A barren world's clock
    /// never moves, so nothing grows back.
    pub(crate) fn advance(&mut self, now: crate::SimTime) {
        if !self.barren {
            self.now = (now.ticks() / 60) as u32;
        }
    }

    pub(crate) fn make_barren(&mut self) {
        self.barren = true;
    }

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
        let remaining = self.remaining.get(&key).map_or(base.capacity, |delta| {
            regrown(delta.remaining, delta.since, base, self.now)
        });
        Ok((remaining > 0).then_some(BaseResource {
            capacity: remaining,
            kind: base.kind,
        }))
    }

    /// What's left of the generated feature `base` at `position` (already
    /// looked up), grown back to now.
    pub(crate) fn remaining_of(
        &self,
        position: WorldPosition,
        base: BaseResource,
    ) -> Option<BaseResource> {
        let remaining = CompactFeaturePosition::checked(position)
            .and_then(|key| self.remaining.get(&key))
            .map_or(base.capacity, |delta| {
                regrown(delta.remaining, delta.since, base, self.now)
            });
        (remaining > 0).then_some(BaseResource {
            capacity: remaining,
            kind: base.kind,
        })
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
        let remaining = current.capacity - u16::from(gathered);
        // A tree chopped down to nothing is felled: it doesn't grow back.
        let felled = remaining == 0 && current.kind == Material::Wood;
        self.remaining.insert(
            key,
            Delta {
                remaining,
                since: if felled { DEAD } else { self.now },
            },
        );
        Ok(Some((current.kind, gathered)))
    }

    pub(crate) fn len(&self) -> usize {
        self.remaining.len()
    }

    /// Marks a generated feature as already used up.
    pub(crate) fn strip(&mut self, position: WorldPosition) {
        let key = CompactFeaturePosition::checked(position)
            .expect("resident world positions fit the finite-world compact envelope");
        self.remaining.insert(
            key,
            Delta {
                remaining: 0,
                since: self.now,
            },
        );
    }

    /// The plant at `position` died: it bears nothing ever again.
    pub(crate) fn kill(&mut self, position: WorldPosition) {
        let key = CompactFeaturePosition::checked(position)
            .expect("resident world positions fit the finite-world compact envelope");
        self.remaining.insert(
            key,
            Delta {
                remaining: 0,
                since: DEAD,
            },
        );
    }

    /// Whether the generated plant at `position` has died or been felled.
    pub(crate) fn is_dead(&self, position: WorldPosition) -> bool {
        CompactFeaturePosition::checked(position)
            .and_then(|key| self.remaining.get(&key))
            .is_some_and(|delta| delta.since == DEAD)
    }

    /// Generated plants that have died or been felled.
    pub(crate) fn dead(&self) -> impl Iterator<Item = WorldPosition> + '_ {
        self.remaining
            .iter()
            .filter(|(_, delta)| delta.since == DEAD)
            .map(|(position, _)| WorldPosition {
                x: i64::from(position.x),
                y: i64::from(position.y),
            })
    }

    /// Whether nothing grows back (a famine valley).
    pub(crate) const fn barren(&self) -> bool {
        self.barren
    }

    pub(crate) fn views<'a>(
        &'a self,
        world: &'a World,
    ) -> impl Iterator<Item = ResourceDeltaView> + 'a {
        self.remaining.iter().map(|(position, &delta)| {
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
                remaining: regrown(delta.remaining, delta.since, resource, self.now),
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
        assert_eq!(size_of::<ResourceDelta>(), 12);
        assert_eq!(align_of::<ResourceDelta>(), 4);
        assert_eq!(size_of::<Delta>(), 8);
    }

    #[test]
    fn food_rots_a_little_each_night_and_cooked_meat_keeps_longer() {
        let mut carried = InventoryView::of(&[
            (Material::Berries, 8),
            (Material::Meat, 6),
            (Material::Roast, 6),
            (Material::Wood, 5),
        ]);
        let lost = carried.rot(1);
        assert_eq!(carried.amount(Material::Wood), 5, "wood keeps");
        assert!(carried.amount(Material::Meat) < carried.amount(Material::Roast));
        assert_eq!(
            lost,
            8 + 6 + 6
                - carried.amount(Material::Berries) as u32
                - carried.amount(Material::Meat) as u32
                - carried.amount(Material::Roast) as u32
        );
        let mut stored = InventoryView::of(&[(Material::Berries, 8)]);
        stored.rot(3);
        let mut out = InventoryView::of(&[(Material::Berries, 8)]);
        out.rot(1);
        assert!(
            stored.amount(Material::Berries) >= out.amount(Material::Berries),
            "huts keep food longer"
        );
    }

    #[test]
    fn a_basket_holds_more_and_tools_are_kept() {
        let hands = InventoryView::of(&[(Material::Wood, CARRY_CAPACITY)]);
        assert!(!hands.can_add(Material::Berries));
        let basket =
            InventoryView::of(&[(Material::Wood, CARRY_CAPACITY - 1), (Material::Basket, 1)]);
        assert_eq!(
            basket.capacity(),
            CARRY_CAPACITY + Material::Basket.properties().room
        );
        assert!(basket.can_add(Material::Berries));
        for kept in [
            Material::Blade,
            Material::Hide,
            Material::Spear,
            Material::Basket,
        ] {
            assert!(kept.properties().worn(), "{kept:?} stays on the body");
        }
        assert!(!Material::Wood.properties().worn());
    }

    #[test]
    fn inventory_capacity_is_one_load_shared_by_every_material() {
        let inventory = InventoryView::of(&[(Material::Berries, 7), (Material::Wood, 4)]);
        assert_eq!(inventory.total(), 11);
        assert_eq!(inventory.remaining_capacity(Material::Berries), 1);
        assert_eq!(inventory.remaining_capacity(Material::Stone), 1);
        assert!(!InventoryView::of(&[(Material::Wood, CARRY_CAPACITY)]).can_add(Material::Stone));
        assert_eq!(
            InventoryView::of(&[(Material::Berries, u8::MAX)])
                .remaining_capacity(Material::Berries),
            0
        );
    }
}
