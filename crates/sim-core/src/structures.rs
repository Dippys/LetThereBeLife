use std::{collections::BTreeMap, error::Error, fmt};

use crate::{AgentId, SimTime, WorldPosition, agent::CompactPosition};

pub const SHELTER_WOOD_COST: u8 = 8;
pub const SHELTER_STONE_COST: u8 = 0;
pub const SHELTER_BUILD_TICKS: u64 = 600;
pub const HEARTH_STONE_COST: u8 = 3;
pub const HEARTH_WOOD_COST: u8 = 2;
pub const HEARTH_BUILD_TICKS: u64 = 300;
/// How far around a building site `would_enclose` looks for a way out.
const ENCLOSURE_RADIUS: i64 = 4;

/// Whether putting a structure on `site` would shut someone in: some walkable
/// cell next to it could get out of the surrounding area before, and can't
/// once the site is blocked (people move in the four directions). `open` says
/// whether a cell is walkable now; pockets that were already closed don't count.
pub(crate) fn would_enclose(site: WorldPosition, open: impl Fn(WorldPosition) -> bool) -> bool {
    const SIDE: usize = (2 * ENCLOSURE_RADIUS + 1) as usize;
    let index = |cell: WorldPosition| {
        ((cell.y - site.y + ENCLOSURE_RADIUS) as usize) * SIDE
            + (cell.x - site.x + ENCLOSURE_RADIUS) as usize
    };
    let distance = |cell: WorldPosition| cell.x.abs_diff(site.x).max(cell.y.abs_diff(site.y));
    let escapes = |start: WorldPosition, blocked: bool| {
        let mut seen = [false; SIDE * SIDE];
        let mut frontier = vec![start];
        seen[index(start)] = true;
        while let Some(cell) = frontier.pop() {
            if distance(cell) == ENCLOSURE_RADIUS as u64 {
                return true;
            }
            for (dx, dy) in [(0, -1), (-1, 0), (1, 0), (0, 1)] {
                let next = WorldPosition {
                    x: cell.x + dx,
                    y: cell.y + dy,
                };
                if distance(next) > ENCLOSURE_RADIUS as u64
                    || seen[index(next)]
                    || (blocked && next == site)
                    || !(next == site || open(next))
                {
                    continue;
                }
                seen[index(next)] = true;
                frontier.push(next);
            }
        }
        false
    };
    (-1..=1)
        .flat_map(|dy| (-1..=1).map(move |dx| (dx, dy)))
        .filter(|&offset| offset != (0, 0))
        .map(|(dx, dy)| WorldPosition {
            x: site.x + dx,
            y: site.y + dy,
        })
        .any(|cell| open(cell) && escapes(cell, false) && !escapes(cell, true))
}

/// A fire holds at most this much fuel (seconds of burning) at once.
pub const MAX_FUEL_SECONDS: u32 = 2 * 3_600;
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

    /// Whether it needs fuel to work (a fire).
    pub const fn burns(self) -> bool {
        matches!(self.purpose(), Purpose::Warmth)
    }

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
    /// For a fire: the simulated second it burns out (0 if it never burned).
    pub fuel_until: u32,
}

impl StructureView {
    /// Finished and, if it's a fire, burning at simulated second `now`.
    pub const fn working(self, now: u32) -> bool {
        matches!(self.state, StructureState::Complete)
            && (!self.kind.burns() || self.fuel_until > now)
    }
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
    /// It would shut a walkable cell nearby off from everywhere else.
    WouldEnclose,
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
    /// For a fire: the simulated second it burns out.
    fuel_until: u32,
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
            fuel_until: self.fuel_until,
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
            fuel_until: 0,
        };
        self.records.push(Some(record));
        self.by_position.insert((position.y, position.x), id);
        self.by_builder.insert(builder, id);
        self.live_count += 1;
        Ok(record.view(id))
    }

    /// Finishes `builder`'s structure; a fire starts burning on the fuel built
    /// into it (`fuel_until`).
    pub(crate) fn complete_for_builder(
        &mut self,
        builder: AgentId,
        fuel_until: u32,
    ) -> Option<StructureView> {
        let id = self.by_builder.remove(&builder)?;
        let record = self.records.get_mut(id.0 as usize)?.as_mut()?;
        record.state = StructureState::Complete;
        if record.kind.burns() {
            record.fuel_until = fuel_until;
        }
        Some(record.view(id))
    }

    /// Adds `seconds` of burning to the fire at `position` (relighting it if
    /// it's out), up to `MAX_FUEL_SECONDS` ahead of `now`. Returns whether
    /// there was a finished fire there.
    pub(crate) fn add_fuel(&mut self, position: WorldPosition, seconds: u32, now: u32) -> bool {
        let Some(id) = self.structure_at(position) else {
            return false;
        };
        let Some(record) = self.records.get_mut(id.0 as usize).and_then(Option::as_mut) else {
            return false;
        };
        if !record.kind.burns() || record.state != StructureState::Complete {
            return false;
        }
        record.fuel_until = (record.fuel_until.max(now) + seconds).min(now + MAX_FUEL_SECONDS);
        true
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

    /// A burning fire within one cell of `position` at simulated second `now`.
    pub(crate) fn fire_beside(&self, position: WorldPosition, now: u32) -> bool {
        self.structure_beside(position, |view| view.kind.burns() && view.working(now))
    }

    /// A finished fire within one cell of `position`, burning or not.
    pub(crate) fn hearth_position_beside(&self, position: WorldPosition) -> Option<WorldPosition> {
        (-1..=1)
            .flat_map(|dy| (-1..=1).map(move |dx| (dx, dy)))
            .map(|(dx, dy)| WorldPosition {
                x: position.x + dx,
                y: position.y + dy,
            })
            .find(|&cell| {
                self.structure_at(cell)
                    .and_then(|id| self.view(id))
                    .is_some_and(|view| view.kind.burns() && view.state == StructureState::Complete)
            })
    }

    fn structure_beside(
        &self,
        position: WorldPosition,
        wanted: impl Fn(StructureView) -> bool,
    ) -> bool {
        (-1..=1).any(|dy| {
            (-1..=1).any(|dx| {
                self.structure_at(WorldPosition {
                    x: position.x + dx,
                    y: position.y + dy,
                })
                .and_then(|id| self.view(id))
                .is_some_and(&wanted)
            })
        })
    }

    #[cfg(test)]
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
    }

    #[test]
    fn a_site_that_would_seal_a_cell_in_is_refused() {
        let at = |x, y| WorldPosition { x, y };
        // A one-cell nook: water all around (0, 0) except its way out at (1, 0).
        let water = [
            at(-1, -1),
            at(0, -1),
            at(1, -1),
            at(-1, 0),
            at(-1, 1),
            at(0, 1),
            at(1, 1),
        ];
        let open = |cell: WorldPosition| !water.contains(&cell);
        assert!(would_enclose(at(1, 0), open), "blocking the nook's mouth");
        assert!(!would_enclose(at(3, 3), open), "open ground elsewhere");
        // A nook that is already closed off isn't this site's doing.
        let closed = |cell: WorldPosition| !water.contains(&cell) && cell != at(1, 0);
        assert!(!would_enclose(at(2, 0), closed));
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
