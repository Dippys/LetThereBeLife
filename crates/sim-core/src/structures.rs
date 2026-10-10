use std::{collections::BTreeMap, error::Error, fmt};

use crate::{AgentId, SimTime, WorldPosition, agent::CompactPosition};

pub const SHELTER_WOOD_COST: u8 = 8;
pub const SHELTER_STONE_COST: u8 = 0;
pub const SHELTER_BUILD_TICKS: u64 = 600;
pub const HEARTH_STONE_COST: u8 = 3;
pub const HEARTH_WOOD_COST: u8 = 2;
pub const HEARTH_BUILD_TICKS: u64 = 300;
/// Cold relieved by one warm-up at a hearth.
pub const HEARTH_WARMTH: u16 = 3_500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct StructureId(u32);

impl StructureId {
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum StructureKind {
    /// A lean-to: sleeping beside it keeps the cold off.
    Shelter = 0,
    /// A ring of stones around a fire: standing by it warms you.
    Hearth = 1,
}

/// What a structure is for, which is also how people show it to others.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// A place to sleep out of the wind.
    Rest,
    /// A place to get warm.
    Warmth,
}

impl StructureKind {
    pub const COUNT: usize = 2;
    pub const ALL: [Self; Self::COUNT] = [Self::Shelter, Self::Hearth];

    pub const fn purpose(self) -> Purpose {
        match self {
            Self::Shelter => Purpose::Rest,
            Self::Hearth => Purpose::Warmth,
        }
    }

    /// What it takes to build: materials and amounts.
    pub const fn cost(self) -> [(crate::Material, u8); 2] {
        match self {
            Self::Shelter => [
                (crate::Material::Wood, SHELTER_WOOD_COST),
                (crate::Material::Stone, SHELTER_STONE_COST),
            ],
            Self::Hearth => [
                (crate::Material::Stone, HEARTH_STONE_COST),
                (crate::Material::Wood, HEARTH_WOOD_COST),
            ],
        }
    }

    pub const fn build_ticks(self) -> u64 {
        match self {
            Self::Shelter => SHELTER_BUILD_TICKS,
            Self::Hearth => HEARTH_BUILD_TICKS,
        }
    }

    /// Whether `inventory` holds everything this takes.
    pub fn affordable(self, inventory: crate::InventoryView) -> bool {
        self.cost()
            .iter()
            .all(|&(material, amount)| inventory.amount(material) >= amount)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum StructureState {
    UnderConstruction = 0,
    Complete = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StructureView {
    pub id: StructureId,
    pub kind: StructureKind,
    pub state: StructureState,
    pub position: WorldPosition,
    pub builder: Option<AgentId>,
    pub started_at: SimTime,
    pub completes_at: SimTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum StructureDiagnosticKind {
    Started,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StructureDiagnostic {
    pub structure: StructureView,
    pub at: SimTime,
    pub kind: StructureDiagnosticKind,
    pub refunded_wood: u8,
    pub refunded_stone: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildShelterError {
    MissingAgent,
    DeadAgent,
    PolicyControlled,
    AgentCommitted,
    NotCardinallyAdjacent,
    OutsideActiveArea,
    OutsideWorld,
    Unloaded,
    Water,
    BlockingFeature,
    Occupied(AgentId),
    StructureOccupied(StructureId),
    InsufficientMaterials,
    TimeOverflow,
    RescheduleLimit,
    EventSequenceExhausted,
    StructureLimit,
}

impl fmt::Display for BuildShelterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "shelter construction failed: {self:?}")
    }
}

impl Error for BuildShelterError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct StructureRecord {
    started_at: SimTime,
    completes_at: SimTime,
    position: CompactPosition,
    builder: AgentId,
    state: StructureState,
    kind: StructureKind,
}

impl StructureRecord {
    fn view(self, id: StructureId) -> StructureView {
        StructureView {
            id,
            kind: self.kind,
            state: self.state,
            position: self.position.world(),
            builder: (self.state == StructureState::UnderConstruction).then_some(self.builder),
            started_at: self.started_at,
            completes_at: self.completes_at,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct StructureStore {
    records: Vec<Option<StructureRecord>>,
    by_position: BTreeMap<(i16, i16), StructureId>,
    by_builder: BTreeMap<AgentId, StructureId>,
    live_count: usize,
}

impl StructureStore {
    pub(crate) fn can_start(
        &self,
        builder: AgentId,
        position: WorldPosition,
    ) -> Result<(), BuildShelterError> {
        if self.structure_at(position).is_some() {
            return Err(BuildShelterError::StructureOccupied(
                self.structure_at(position).expect("checked above"),
            ));
        }
        if self.by_builder.contains_key(&builder) {
            return Err(BuildShelterError::AgentCommitted);
        }
        if self.records.len() >= u32::MAX as usize {
            return Err(BuildShelterError::StructureLimit);
        }
        Ok(())
    }

    pub(crate) fn structure_at(&self, position: WorldPosition) -> Option<StructureId> {
        CompactPosition::checked(position)
            .and_then(|key| self.by_position.get(&(key.y, key.x)).copied())
    }

    pub(crate) fn view(&self, id: StructureId) -> Option<StructureView> {
        self.records
            .get(id.0 as usize)
            .copied()
            .flatten()
            .map(|record| record.view(id))
    }

    pub(crate) fn start(
        &mut self,
        builder: AgentId,
        position: WorldPosition,
        kind: StructureKind,
        started_at: SimTime,
        completes_at: SimTime,
    ) -> Result<StructureView, BuildShelterError> {
        let position = CompactPosition::checked(position).ok_or(BuildShelterError::OutsideWorld)?;
        if let Some(id) = self.by_position.get(&(position.y, position.x)).copied() {
            return Err(BuildShelterError::StructureOccupied(id));
        }
        if self.by_builder.contains_key(&builder) {
            return Err(BuildShelterError::AgentCommitted);
        }
        let raw =
            u32::try_from(self.records.len()).map_err(|_| BuildShelterError::StructureLimit)?;
        let id = StructureId(raw);
        let record = StructureRecord {
            started_at,
            completes_at,
            position,
            builder,
            state: StructureState::UnderConstruction,
            kind,
        };
        self.records.push(Some(record));
        self.by_position.insert((position.y, position.x), id);
        self.by_builder.insert(builder, id);
        self.live_count += 1;
        Ok(record.view(id))
    }

    pub(crate) fn complete_for_builder(&mut self, builder: AgentId) -> Option<StructureView> {
        let id = self.by_builder.remove(&builder)?;
        let record = self.records.get_mut(id.0 as usize)?.as_mut()?;
        record.state = StructureState::Complete;
        Some(record.view(id))
    }

    pub(crate) fn cancel_for_builder(&mut self, builder: AgentId) -> Option<StructureView> {
        let id = self.by_builder.remove(&builder)?;
        let record = self.records.get_mut(id.0 as usize)?.take()?;
        self.by_position
            .remove(&(record.position.y, record.position.x));
        self.live_count -= 1;
        Some(record.view(id))
    }

    pub(crate) fn is_sheltered_access(&self, position: WorldPosition) -> bool {
        cardinal_neighbors(position).any(|candidate| {
            self.structure_at(candidate)
                .and_then(|id| self.view(id))
                .is_some_and(|view| {
                    view.state == StructureState::Complete && view.kind == StructureKind::Shelter
                })
        })
    }

    /// Whether a finished hearth stands within one cell of `position`.
    pub(crate) fn hearth_beside(&self, position: WorldPosition) -> bool {
        (-1..=1).any(|dy| {
            (-1..=1).any(|dx| {
                self.structure_at(WorldPosition {
                    x: position.x + dx,
                    y: position.y + dy,
                })
                .and_then(|id| self.view(id))
                .is_some_and(|view| {
                    view.state == StructureState::Complete && view.kind == StructureKind::Hearth
                })
            })
        })
    }

    pub(crate) fn push_views_in(&self, area: crate::WorldRect, output: &mut Vec<StructureView>) {
        for y in area.min.y..area.max.y {
            let Ok(y) = i16::try_from(y) else { continue };
            let min_x = i16::try_from(area.min.x).unwrap_or(i16::MIN);
            let max_x = i16::try_from(area.max.x - 1).unwrap_or(i16::MAX);
            output.extend(
                self.by_position
                    .range((y, min_x)..=(y, max_x))
                    .filter_map(|(_, id)| self.view(*id)),
            );
        }
    }

    pub(crate) fn views(&self, limit: usize) -> impl Iterator<Item = StructureView> + '_ {
        self.records
            .iter()
            .enumerate()
            .filter_map(|(raw, record)| record.map(|record| record.view(StructureId(raw as u32))))
            .take(limit)
    }

    pub(crate) const fn len(&self) -> usize {
        self.live_count
    }

    pub(crate) fn retained_slots(&self) -> usize {
        self.records.capacity()
    }
}

fn cardinal_neighbors(position: WorldPosition) -> std::array::IntoIter<WorldPosition, 4> {
    [
        WorldPosition {
            x: position.x,
            y: position.y - 1,
        },
        WorldPosition {
            x: position.x - 1,
            y: position.y,
        },
        WorldPosition {
            x: position.x + 1,
            y: position.y,
        },
        WorldPosition {
            x: position.x,
            y: position.y + 1,
        },
    ]
    .into_iter()
}

#[cfg(test)]
mod tests {
    use std::mem::{align_of, size_of};

    use super::*;

    #[test]
    fn structure_records_and_public_domains_are_compact() {
        assert_eq!(size_of::<StructureId>(), 4);
        assert_eq!(size_of::<StructureKind>(), 1);
        assert_eq!(size_of::<StructureState>(), 1);
        assert_eq!(size_of::<StructureRecord>(), 32);
        assert_eq!(align_of::<StructureRecord>(), 8);
    }

    #[test]
    fn cancellation_releases_position_without_reusing_identity() {
        let mut store = StructureStore::default();
        let first = store
            .start(
                AgentId::new(0),
                WorldPosition { x: 1, y: 0 },
                StructureKind::Shelter,
                SimTime::ZERO,
                SimTime::from_ticks(600),
            )
            .unwrap();
        assert_eq!(store.cancel_for_builder(AgentId::new(0)), Some(first));
        let second = store
            .start(
                AgentId::new(0),
                WorldPosition { x: 1, y: 0 },
                StructureKind::Shelter,
                SimTime::ZERO,
                SimTime::from_ticks(600),
            )
            .unwrap();
        assert!(second.id.get() > first.id.get());
        assert_eq!(store.len(), 1);
    }
}
