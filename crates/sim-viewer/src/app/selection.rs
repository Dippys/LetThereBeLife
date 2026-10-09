//! Right-drag generation selection bounds and cached selection validity.

use sim_core::{WORLD_GENERATION_BOUNDS, World, WorldPosition, WorldRect};

use super::{SelectionValidation, ViewerApp};

impl ViewerApp {
    pub(super) fn selection_preview_is_valid(&mut self) -> bool {
        let Some(bounds) = self.selection else {
            self.selection_validation = None;
            return true;
        };
        let world_revision = self.engine.world().revision();
        if let Some(cached) = self.selection_validation
            && cached.bounds == bounds
            && cached.world_revision == world_revision
        {
            return cached.valid;
        }
        let valid = selection_is_valid(self.engine.world(), Some(bounds));
        self.selection_validation = Some(SelectionValidation {
            bounds,
            world_revision,
            valid,
        });
        valid
    }
}

pub(super) fn selection_is_valid(world: &World, selection: Option<WorldRect>) -> bool {
    selection.is_none_or(|bounds| world.validate_generation_request(bounds).is_ok())
}

pub(super) fn bounded_selection(start: WorldPosition, current: WorldPosition) -> Option<WorldRect> {
    WORLD_GENERATION_BOUNDS
        .contains(start)
        .then(|| WorldRect::from_inclusive_points(start, current))?
        .intersection(WORLD_GENERATION_BOUNDS)
}
