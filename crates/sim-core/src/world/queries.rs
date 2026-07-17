use super::{
    BaseResource, ClimateSample, Feature, MAX_TRAVERSABLE_ELEVATION_DELTA, Standability,
    TerrainCell, TraversalKind, TraversalStep, WORLD_GENERATION_BOUNDS, WaterSource, World,
    WorldPosition, WorldQueryError, WorldRect, chunk_coord, climate_at, surface_traversal_cost,
    water_source,
};

impl World {
    /// Validates one resident standing position using the same physical rules
    /// as cardinal traversal targets, without requiring an artificial step.
    pub fn standability_at(
        &self,
        position: WorldPosition,
    ) -> Result<Standability, WorldQueryError> {
        let cell = self.resident_cell(position)?;
        if water_source(cell).is_some() {
            return Ok(Standability::BlockedByWater);
        }
        Ok(Standability::Standable)
    }

    /// Finds a sparse surface feature without scanning the complete feature list.
    pub fn feature_at(&self, position: WorldPosition) -> Option<Feature> {
        let coord = chunk_coord(position);
        self.chunks.get(&coord).and_then(|chunk| {
            chunk.bounds(coord).contains(position).then(|| {
                chunk
                    .features()
                    .binary_search_by_key(&(position.y, position.x), |feature| {
                        (feature.position.y, feature.position.x)
                    })
                    .ok()
                    .and_then(|index| chunk.features().get(index))
                    .copied()
            })?
        })
    }

    /// Returns immutable generated capacity without inventing depletion state.
    pub fn base_resource_at(&self, position: WorldPosition) -> Option<BaseResource> {
        self.feature_at(position).map(Feature::base_resource)
    }

    pub fn resource_at(
        &self,
        position: WorldPosition,
    ) -> Result<Option<BaseResource>, WorldQueryError> {
        self.resident_cell(position)?;
        Ok(self.base_resource_at(position))
    }

    pub fn water_at(
        &self,
        position: WorldPosition,
    ) -> Result<Option<WaterSource>, WorldQueryError> {
        let cell = self.resident_cell(position)?;
        Ok(water_source(cell))
    }

    /// Derives one cardinal walking step without storing pathfinding flags.
    pub fn traversal_step(
        &self,
        from: WorldPosition,
        to: WorldPosition,
    ) -> Result<TraversalStep, WorldQueryError> {
        let dx = i128::from(to.x) - i128::from(from.x);
        let dy = i128::from(to.y) - i128::from(from.y);
        if dx.abs() + dy.abs() != 1 {
            return Err(WorldQueryError::NonCardinalStep);
        }
        let source = self.resident_cell(from)?;
        let target = self.resident_cell(to)?;
        let elevation_delta = i32::from(target.elevation) - i32::from(source.elevation);
        let kind = if water_source(source).is_some() || water_source(target).is_some() {
            TraversalKind::BlockedByWater
        } else if elevation_delta.unsigned_abs() > u32::from(MAX_TRAVERSABLE_ELEVATION_DELTA) {
            TraversalKind::BlockedBySlope
        } else {
            TraversalKind::Passable
        };
        let cost = if kind == TraversalKind::Passable {
            surface_traversal_cost(target.surface())
                + (elevation_delta.unsigned_abs() as u16).div_ceil(32)
        } else {
            0
        };
        Ok(TraversalStep {
            elevation_delta,
            cost,
            kind,
        })
    }

    pub fn cell(&self, position: WorldPosition) -> Option<TerrainCell> {
        let coord = chunk_coord(position);
        self.chunks
            .get(&coord)
            .and_then(|chunk| chunk.cell(coord, position))
    }

    fn resident_cell(&self, position: WorldPosition) -> Result<TerrainCell, WorldQueryError> {
        if !WORLD_GENERATION_BOUNDS.contains(position) {
            return Err(WorldQueryError::OutsideWorldBounds);
        }
        self.cell(position).ok_or(WorldQueryError::Unloaded)
    }

    pub fn climate_at(&self, position: WorldPosition) -> Option<ClimateSample> {
        let cell = self.cell(position)?;
        Some(climate_at(self.seed, position.x, position.y, cell.moisture))
    }

    pub fn loaded_bounds_at(&self, position: WorldPosition) -> Option<WorldRect> {
        let coord = chunk_coord(position);
        self.chunks.get(&coord).and_then(|chunk| {
            let bounds = chunk.bounds(coord);
            bounds.contains(position).then_some(bounds)
        })
    }
}
