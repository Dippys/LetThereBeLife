use super::{
    ChunkCoord, Feature, TerrainCell, World, WorldPosition, WorldRect, visit_loaded_chunk_region,
};

impl World {
    /// Iterates resident cells in deterministic `ChunkCoord` order (`x`, then
    /// `y`), then in row-major order within each tile.
    ///
    /// This intentionally does not promise global world-row order, because
    /// sparse retained tiles need not form a dense rectangle.
    pub fn cells(&self) -> impl Iterator<Item = (WorldPosition, TerrainCell)> + '_ {
        self.chunks.iter().flat_map(|(&coord, chunk)| {
            let bounds = chunk.bounds(coord);
            let width = (bounds.max.x - bounds.min.x) as usize;
            (0..chunk.cell_count()).map(move |index| {
                let position = WorldPosition {
                    x: bounds.min.x + (index % width) as i64,
                    y: bounds.min.y + (index / width) as i64,
                };
                (
                    position,
                    chunk
                        .cell(coord, position)
                        .expect("stored chunk coverage must contain its cells"),
                )
            })
        })
    }

    /// Iterates resident sparse features in deterministic tile order.
    pub fn all_features(&self) -> impl Iterator<Item = &Feature> {
        self.chunks.iter().flat_map(|(&coord, chunk)| {
            let coverage = chunk.bounds(coord);
            chunk
                .features()
                .iter()
                .filter(move |feature| coverage.contains(feature.position))
        })
    }

    pub fn features_in(&self, bounds: WorldRect) -> impl Iterator<Item = &Feature> {
        self.all_features()
            .filter(move |feature| bounds.contains(feature.position))
    }

    pub fn visit_cells_in(
        &self,
        bounds: WorldRect,
        visitor: impl FnMut(WorldPosition, TerrainCell),
    ) {
        self.visit_cells_in_step(bounds, 1, visitor);
    }

    pub fn visit_cells_in_step(
        &self,
        bounds: WorldRect,
        step: u32,
        mut visitor: impl FnMut(WorldPosition, TerrainCell),
    ) {
        if bounds.max.x <= bounds.min.x || bounds.max.y <= bounds.min.y {
            return;
        }
        let step = i64::from(step.max(1));
        for (&coord, chunk) in &self.chunks {
            visit_loaded_chunk_region(chunk, coord, bounds, step, &mut visitor);
        }
    }

    pub fn visit_features_in(&self, bounds: WorldRect, mut visitor: impl FnMut(&Feature)) {
        if bounds.max.x <= bounds.min.x || bounds.max.y <= bounds.min.y {
            return;
        }
        for (&coord, chunk) in &self.chunks {
            let coverage = chunk.bounds(coord);
            for feature in chunk.features().iter().filter(|feature| {
                coverage.contains(feature.position) && bounds.contains(feature.position)
            }) {
                visitor(feature);
            }
        }
    }

    /// Visits exact resident chunk coverage intersecting `bounds` in stable
    /// chunk-coordinate order without exposing world storage.
    pub fn visit_loaded_regions_in(
        &self,
        bounds: WorldRect,
        mut visitor: impl FnMut(ChunkCoord, WorldRect),
    ) {
        if bounds.max.x <= bounds.min.x || bounds.max.y <= bounds.min.y {
            return;
        }
        for (&coord, chunk) in &self.chunks {
            let coverage = chunk.bounds(coord);
            if coverage.intersects(bounds) {
                visitor(coord, coverage);
            }
        }
    }

    /// Visits every resident cell in one known chunk without scanning other entries.
    pub fn visit_cells_in_chunk(
        &self,
        coord: ChunkCoord,
        mut visitor: impl FnMut(WorldPosition, TerrainCell),
    ) -> Option<WorldRect> {
        let chunk = self.chunks.get(&coord)?;
        let coverage = chunk.bounds(coord);
        visit_loaded_chunk_region(chunk, coord, coverage, 1, &mut visitor);
        Some(coverage)
    }

    /// Visits sparse features in one known resident chunk without scanning other entries.
    pub fn visit_features_in_chunk(
        &self,
        coord: ChunkCoord,
        mut visitor: impl FnMut(&Feature),
    ) -> Option<WorldRect> {
        let chunk = self.chunks.get(&coord)?;
        let coverage = chunk.bounds(coord);
        for feature in chunk
            .features()
            .iter()
            .filter(|feature| coverage.contains(feature.position))
        {
            visitor(feature);
        }
        Some(coverage)
    }
}
