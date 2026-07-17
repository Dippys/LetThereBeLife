use std::{collections::BTreeMap, error::Error, fmt};

use crate::{
    BaseResource, FeatureKind, ResourceKind, Standability, TraversalKind, TraversalStep,
    WaterSource, World, WorldPosition, WorldQueryError,
};

/// Object kinds that an explicit simulation command can place into resident terrain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SpawnKind {
    Tree,
    BerryBush,
    Rock,
    Water,
}

impl SpawnKind {
    pub const ALL: [Self; 4] = [Self::Tree, Self::BerryBush, Self::Rock, Self::Water];

    pub const fn resource(self) -> Option<BaseResource> {
        match self {
            Self::Tree => Some(FeatureKind::Tree.base_resource()),
            Self::BerryBush => Some(FeatureKind::BerryBush.base_resource()),
            Self::Rock => Some(FeatureKind::Rock.base_resource()),
            Self::Water => None,
        }
    }

    pub const fn feature(self) -> Option<FeatureKind> {
        match self {
            Self::Tree => Some(FeatureKind::Tree),
            Self::BerryBush => Some(FeatureKind::BerryBush),
            Self::Rock => Some(FeatureKind::Rock),
            Self::Water => None,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
struct SpawnedObject {
    remaining: u16,
    kind: SpawnKind,
}

#[derive(Debug, Default)]
pub(crate) struct SpawnedObjects {
    objects: BTreeMap<CompactSpawnPosition, SpawnedObject>,
    revision: u64,
}

impl SpawnedObjects {
    pub(crate) fn insert(&mut self, position: WorldPosition, kind: SpawnKind) {
        let key = CompactSpawnPosition::checked(position)
            .expect("validated finite-world positions fit the compact spawn envelope");
        let remaining = kind.resource().map_or(0, |resource| resource.capacity);
        let replaced = self.objects.insert(key, SpawnedObject { remaining, kind });
        debug_assert!(
            replaced.is_none(),
            "placement validation rejects replacement"
        );
        self.revision = self.revision.saturating_add(1);
    }

    pub(crate) fn at(&self, position: WorldPosition) -> Option<SpawnedObjectView> {
        let key = CompactSpawnPosition::checked(position)?;
        self.objects.get(&key).map(|object| SpawnedObjectView {
            position,
            kind: object.kind,
            remaining: object.kind.resource().map(|_| object.remaining),
        })
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
            Some(SpawnKind::Tree | SpawnKind::Rock) => Standability::BlockedByFeature,
            Some(SpawnKind::Water) => Standability::BlockedByWater,
            Some(SpawnKind::BerryBush) | None => Standability::Standable,
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
        let kind = match self.at(to).map(|object| object.kind) {
            Some(SpawnKind::Tree | SpawnKind::Rock) => TraversalKind::BlockedByFeature,
            Some(SpawnKind::Water) => TraversalKind::BlockedByWater,
            Some(SpawnKind::BerryBush) | None => return Ok(step),
        };
        Ok(TraversalStep::blocked(step.elevation_delta(), kind))
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

    pub(crate) fn gather(
        &mut self,
        position: WorldPosition,
        maximum: u8,
    ) -> Option<(ResourceKind, u8)> {
        let key = CompactSpawnPosition::checked(position)?;
        let object = self.objects.get_mut(&key)?;
        let resource = object.kind.resource()?;
        let gathered = object.remaining.min(u16::from(maximum)) as u8;
        if gathered == 0 {
            return None;
        }
        object.remaining -= u16::from(gathered);
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
                remaining: object.kind.resource().map(|_| object.remaining),
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
        assert_eq!(size_of::<SpawnedObject>(), 4);
        assert_eq!(align_of::<SpawnedObject>(), 2);
    }
}
