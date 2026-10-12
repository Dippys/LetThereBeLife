use std::collections::BTreeMap;

use crate::{AgentId, CHUNK_SIZE, WorldPosition, WorldRect};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
struct SpatialBucket {
    x: i16,
    y: i16,
}

impl SpatialBucket {
    fn at(position: WorldPosition) -> Self {
        Self {
            x: position.x.div_euclid(CHUNK_SIZE) as i16,
            y: position.y.div_euclid(CHUNK_SIZE) as i16,
        }
    }

    fn local_cell(self, position: WorldPosition) -> u16 {
        let x = position.x.rem_euclid(CHUNK_SIZE) as u16;
        let y = position.y.rem_euclid(CHUNK_SIZE) as u16;
        y * CHUNK_SIZE as u16 + x
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(C)]
pub(crate) struct CellOccupant {
    local_cell: u16,
    agent: AgentId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransferError {
    SourceMismatch,
}

/// Sparse authoritative agent positions grouped by the world's existing 64-cell chunks.
///
/// Entries are ordered by cell and then agent so multiple living agents can share a cell
/// without losing deterministic perception or source-validated movement.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct SpatialIndex {
    buckets: BTreeMap<SpatialBucket, Vec<CellOccupant>>,
}

impl SpatialIndex {
    pub(crate) fn from_positions(
        positions: impl IntoIterator<Item = (AgentId, WorldPosition)>,
    ) -> Self {
        let mut index = Self::default();
        for (agent, position) in positions {
            let inserted = index.insert(agent, position);
            debug_assert!(inserted, "population positions were validated as unique");
        }
        index
    }

    pub(crate) fn insert(&mut self, agent: AgentId, position: WorldPosition) -> bool {
        let bucket = SpatialBucket::at(position);
        let local_cell = bucket.local_cell(position);
        let occupants = self.buckets.entry(bucket).or_default();
        match occupants.binary_search_by_key(&(local_cell, agent), |entry| {
            (entry.local_cell, entry.agent)
        }) {
            Ok(_) => false,
            Err(index) => {
                occupants.insert(index, CellOccupant { local_cell, agent });
                true
            }
        }
    }

    pub(crate) fn occupant(&self, position: WorldPosition) -> Option<AgentId> {
        let bucket = SpatialBucket::at(position);
        let local_cell = bucket.local_cell(position);
        let occupants = self.buckets.get(&bucket)?;
        let index = occupants.partition_point(|entry| entry.local_cell < local_cell);
        occupants
            .get(index)
            .filter(|entry| entry.local_cell == local_cell)
            .map(|entry| entry.agent)
    }

    pub(crate) fn occupant_except(
        &self,
        position: WorldPosition,
        excluded: AgentId,
    ) -> Option<AgentId> {
        let bucket = SpatialBucket::at(position);
        let local_cell = bucket.local_cell(position);
        let occupants = self.buckets.get(&bucket)?;
        let start = occupants.partition_point(|entry| entry.local_cell < local_cell);
        occupants[start..]
            .iter()
            .take_while(|entry| entry.local_cell == local_cell)
            .find(|entry| entry.agent != excluded)
            .map(|entry| entry.agent)
    }

    pub(crate) fn remove(&mut self, agent: AgentId, position: WorldPosition) -> bool {
        let bucket = SpatialBucket::at(position);
        let local_cell = bucket.local_cell(position);
        let Some(occupants) = self.buckets.get_mut(&bucket) else {
            return false;
        };
        let Ok(index) = occupants.binary_search_by_key(&(local_cell, agent), |entry| {
            (entry.local_cell, entry.agent)
        }) else {
            return false;
        };
        occupants.remove(index);
        if occupants.is_empty() {
            self.buckets.remove(&bucket);
        }
        true
    }

    pub(crate) fn transfer(
        &mut self,
        agent: AgentId,
        from: WorldPosition,
        target: WorldPosition,
    ) -> Result<(), TransferError> {
        let from_bucket = SpatialBucket::at(from);
        let from_cell = from_bucket.local_cell(from);
        let Some(source_entries) = self.buckets.get_mut(&from_bucket) else {
            return Err(TransferError::SourceMismatch);
        };
        let Ok(source_index) = source_entries
            .binary_search_by_key(&(from_cell, agent), |entry| (entry.local_cell, entry.agent))
        else {
            return Err(TransferError::SourceMismatch);
        };
        source_entries.remove(source_index);
        if source_entries.is_empty() {
            self.buckets.remove(&from_bucket);
        }
        if self.insert(agent, target) {
            Ok(())
        } else {
            let restored = self.insert(agent, from);
            debug_assert!(
                restored,
                "source occupancy must remain available during transfer"
            );
            Err(TransferError::SourceMismatch)
        }
    }

    /// The agents in `area`, by row, then column, then id.
    pub(crate) fn agents_in(&self, area: WorldRect, output: &mut Vec<AgentId>) {
        output.clear();
        if area.max.x <= area.min.x || area.max.y <= area.min.y {
            return;
        }
        // Only the buckets the area overlaps, each scanned once.
        let first = SpatialBucket::at(area.min);
        let last = SpatialBucket::at(WorldPosition {
            x: area.max.x - 1,
            y: area.max.y - 1,
        });
        let mut found: Vec<(i64, i64, AgentId)> = Vec::new();
        for bucket_y in first.y..=last.y {
            for bucket_x in first.x..=last.x {
                let bucket = SpatialBucket {
                    x: bucket_x,
                    y: bucket_y,
                };
                let Some(occupants) = self.buckets.get(&bucket) else {
                    continue;
                };
                for entry in occupants {
                    let position = WorldPosition {
                        x: i64::from(bucket_x) * CHUNK_SIZE
                            + i64::from(entry.local_cell) % CHUNK_SIZE,
                        y: i64::from(bucket_y) * CHUNK_SIZE
                            + i64::from(entry.local_cell) / CHUNK_SIZE,
                    };
                    if area.contains(position) {
                        found.push((position.y, position.x, entry.agent));
                    }
                }
            }
        }
        found.sort_unstable();
        output.extend(found.into_iter().map(|(_, _, agent)| agent));
    }

    pub(crate) fn len(&self) -> usize {
        self.buckets.values().map(Vec::len).sum()
    }

    pub(crate) fn retained_entry_capacity(&self) -> usize {
        self.buckets.values().map(Vec::capacity).sum()
    }

    pub(crate) fn bucket_count(&self) -> usize {
        self.buckets.len()
    }
}

#[cfg(test)]
mod tests {
    use std::mem::{align_of, size_of};

    use super::*;

    #[test]
    fn compact_entries_cross_signed_chunk_edges_without_aliasing() {
        assert_eq!(size_of::<SpatialBucket>(), 4);
        assert_eq!(size_of::<CellOccupant>(), 8);
        assert_eq!(align_of::<CellOccupant>(), 4);

        let positions = [
            WorldPosition { x: -65, y: -1 },
            WorldPosition { x: -64, y: 0 },
            WorldPosition { x: -1, y: 63 },
            WorldPosition { x: 0, y: 64 },
            WorldPosition { x: 63, y: 0 },
            WorldPosition { x: 64, y: -64 },
        ];
        let index = SpatialIndex::from_positions(
            positions
                .into_iter()
                .enumerate()
                .map(|(raw, position)| (AgentId::new(raw as u32), position)),
        );
        for (raw, position) in positions.into_iter().enumerate() {
            assert_eq!(index.occupant(position), Some(AgentId::new(raw as u32)));
        }
        assert_eq!(index.len(), positions.len());
    }

    #[test]
    fn shared_cells_preserve_all_agents_and_source_validated_transfers() {
        let left = WorldPosition { x: 63, y: -1 };
        let right = WorldPosition { x: 64, y: -1 };
        let mut index =
            SpatialIndex::from_positions([(AgentId::new(0), left), (AgentId::new(1), right)]);

        assert_eq!(index.transfer(AgentId::new(0), left, right), Ok(()));
        assert_eq!(index.occupant(left), None);
        assert_eq!(index.occupant(right), Some(AgentId::new(0)));
        assert_eq!(
            index.occupant_except(right, AgentId::new(0)),
            Some(AgentId::new(1))
        );
        let mut agents = Vec::new();
        index.agents_in(
            WorldRect {
                min: right,
                max: WorldPosition {
                    x: right.x + 1,
                    y: right.y + 1,
                },
            },
            &mut agents,
        );
        assert_eq!(agents, [AgentId::new(0), AgentId::new(1)]);
        assert_eq!(
            index.transfer(
                AgentId::new(0),
                WorldPosition { x: 62, y: -1 },
                WorldPosition { x: 65, y: -1 },
            ),
            Err(TransferError::SourceMismatch)
        );
        assert_eq!(index.len(), 2);
    }
}
