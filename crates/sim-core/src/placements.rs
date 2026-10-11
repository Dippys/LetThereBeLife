use std::{collections::BTreeMap, error::Error, fmt};

use crate::{
    BaseResource, FeatureKind, Material, Standability, TraversalStep, WaterSource, World,
    WorldPosition, WorldQueryError,
};

/// Object kinds that an explicit simulation command can place into resident terrain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum SpawnKind {
    Tree,
    BerryBush,
    Rock,
    Water,
    BitterBush,
}

impl SpawnKind {
    pub const ALL: [Self; 5] = [
        Self::Tree,
        Self::BerryBush,
        Self::Rock,
        Self::Water,
        Self::BitterBush,
    ];

    pub const fn resource(self) -> Option<BaseResource> {
        match self.feature() {
            Some(feature) => Some(feature.base_resource()),
            None => None,
        }
    }

    pub const fn feature(self) -> Option<FeatureKind> {
        match self {
            Self::Tree => Some(FeatureKind::Tree),
            Self::BerryBush => Some(FeatureKind::BerryBush),
            Self::BitterBush => Some(FeatureKind::BitterBush),
            Self::Rock => Some(FeatureKind::Rock),
            Self::Water => None,
        }
    }

    /// The kind that grows from a plant of `feature` (none for rock).
    pub const fn growing(feature: FeatureKind) -> Option<Self> {
        match feature {
            FeatureKind::Tree => Some(Self::Tree),
            FeatureKind::BerryBush => Some(Self::BerryBush),
            FeatureKind::BitterBush => Some(Self::BitterBush),
            FeatureKind::Rock => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpawnedObjectView {
    pub position: WorldPosition,
    pub kind: SpawnKind,
    pub remaining: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnObjectError {
    OutsideWorld,
    Unloaded,
    Occupied,
    BlockedByStructure,
    ExistingObject,
    BlockedByWater,
    BlockedByFeature,
}

impl fmt::Display for SpawnObjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "object placement failed: {self:?}")
    }
}

impl Error for SpawnObjectError {}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[repr(C)]
struct CompactSpawnPosition {
    y: i16,
    x: i16,
}

impl CompactSpawnPosition {
    fn checked(position: WorldPosition) -> Option<Self> {
        Some(Self {
            y: i16::try_from(position.y).ok()?,
            x: i16::try_from(position.x).ok()?,
        })
    }

    const fn world(self) -> WorldPosition {
        WorldPosition {
            x: self.x as i64,
            y: self.y as i64,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(C)]
struct SpawnedObject {
    /// Simulated second `remaining` was last true (growth counts from here).
    since: u32,
    remaining: u16,
    kind: SpawnKind,
}

impl SpawnedObject {
    /// What it holds at `now`: plants grow back as generated ones do.
    fn remaining(self, now: u32) -> u16 {
        self.kind.resource().map_or(0, |base| {
            crate::resources::regrown(self.remaining, self.since, base, now)
        })
    }
}

/// Placed objects, and plants that grew by themselves.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct SpawnedObjects {
    objects: BTreeMap<CompactSpawnPosition, SpawnedObject>,
    revision: u64,
    /// The engine's current time in seconds, advanced every tick.
    now: u32,
}

impl SpawnedObjects {
    /// Places a full-grown object.
    pub(crate) fn insert(&mut self, position: WorldPosition, kind: SpawnKind) {
        let remaining = kind.resource().map_or(0, |resource| resource.capacity);
        self.plant(position, kind, remaining);
    }

    /// Places an object holding `remaining`, growing from now on.
    pub(crate) fn plant(&mut self, position: WorldPosition, kind: SpawnKind, remaining: u16) {
        let key = CompactSpawnPosition::checked(position)
            .expect("validated finite-world positions fit the compact spawn envelope");
        let since = self.now;
        let replaced = self.objects.insert(
            key,
            SpawnedObject {
                since,
                remaining,
                kind,
            },
        );
        debug_assert!(
            replaced.is_none(),
            "placement validation rejects replacement"
        );
        self.revision = self.revision.saturating_add(1);
    }

    /// Removes the object at `position` (a plant that died or was felled).
    pub(crate) fn remove(&mut self, position: WorldPosition) {
        if let Some(key) = CompactSpawnPosition::checked(position)
            && self.objects.remove(&key).is_some()
        {
            self.revision = self.revision.saturating_add(1);
        }
    }

    /// Moves the clock growth is measured against.
    pub(crate) fn advance(&mut self, now: u32) {
        self.now = now;
    }

    pub(crate) fn at(&self, position: WorldPosition) -> Option<SpawnedObjectView> {
        let key = CompactSpawnPosition::checked(position)?;
        self.objects.get(&key).map(|object| SpawnedObjectView {
            position,
            kind: object.kind,
            remaining: object.kind.resource().map(|_| object.remaining(self.now)),
        })
    }

    pub(crate) fn reserves_exclusive_use_at(&self, world: &World, position: WorldPosition) -> bool {
        world
            .feature_at(position)
            .is_some_and(|feature| matches!(feature.kind, FeatureKind::Tree | FeatureKind::Rock))
            || self
                .at(position)
                .is_some_and(|object| matches!(object.kind, SpawnKind::Tree | SpawnKind::Rock))
    }

    pub(crate) fn standability_at(
        &self,
        world: &World,
        position: WorldPosition,
    ) -> Result<Standability, WorldQueryError> {
        let base = world.standability_at(position)?;
        if base != Standability::Standable {
            return Ok(base);
        }
        Ok(match self.at(position).map(|object| object.kind) {
            Some(SpawnKind::Water) => Standability::BlockedByWater,
            Some(
                SpawnKind::Tree | SpawnKind::BerryBush | SpawnKind::BitterBush | SpawnKind::Rock,
            )
            | None => Standability::Standable,
        })
    }

    pub(crate) fn traversal_step(
        &self,
        world: &World,
        from: WorldPosition,
        to: WorldPosition,
    ) -> Result<TraversalStep, WorldQueryError> {
        let step = world.traversal_step(from, to)?;
        if !step.is_passable() {
            return Ok(step);
        }
        match self.at(to).map(|object| object.kind) {
            // A placed pond is shallow: it can be waded, slowly.
            Some(SpawnKind::Water) => Ok(TraversalStep::wading(step.elevation_delta())),
            Some(
                SpawnKind::Tree | SpawnKind::BerryBush | SpawnKind::BitterBush | SpawnKind::Rock,
            )
            | None => Ok(step),
        }
    }

    pub(crate) fn water_at(
        &self,
        world: &World,
        position: WorldPosition,
    ) -> Result<Option<WaterSource>, WorldQueryError> {
        let base = world.water_at(position)?;
        Ok(base.or_else(|| {
            self.at(position)
                .filter(|object| object.kind == SpawnKind::Water)
                .map(|_| WaterSource::Lake)
        }))
    }

    pub(crate) fn resource_at(&self, position: WorldPosition) -> Option<BaseResource> {
        let object = self.at(position)?;
        let mut resource = object.kind.resource()?;
        resource.capacity = object.remaining?;
        (resource.capacity > 0).then_some(resource)
    }

    /// Picks or chops up to `maximum` from the object at `position`. A tree
    /// chopped down to nothing is felled and gone.
    pub(crate) fn gather(
        &mut self,
        position: WorldPosition,
        maximum: u8,
    ) -> Option<(Material, u8)> {
        let key = CompactSpawnPosition::checked(position)?;
        let now = self.now;
        let object = self.objects.get_mut(&key)?;
        let resource = object.kind.resource()?;
        let remaining = object.remaining(now);
        let gathered = remaining.min(u16::from(maximum)) as u8;
        if gathered == 0 {
            return None;
        }
        object.remaining = remaining - u16::from(gathered);
        object.since = now;
        if object.remaining == 0 && object.kind == SpawnKind::Tree {
            self.objects.remove(&key);
        }
        self.revision = self.revision.saturating_add(1);
        Some((resource.kind, gathered))
    }

    pub(crate) fn len(&self) -> usize {
        self.objects.len()
    }

    pub(crate) const fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn views(&self) -> impl Iterator<Item = SpawnedObjectView> + '_ {
        self.objects
            .iter()
            .map(|(&position, object)| SpawnedObjectView {
                position: position.world(),
                kind: object.kind,
                remaining: object.kind.resource().map(|_| object.remaining(self.now)),
            })
    }
}

#[cfg(test)]
mod tests {
    use std::mem::{align_of, size_of};

    use super::*;

    #[test]
    fn spawned_object_payload_is_compact() {
        assert_eq!(size_of::<CompactSpawnPosition>(), 4);
        assert_eq!(align_of::<CompactSpawnPosition>(), 2);
        assert_eq!(size_of::<SpawnedObject>(), 8);
        assert_eq!(align_of::<SpawnedObject>(), 4);
    }
}
