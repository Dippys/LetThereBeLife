//! Far sight: beyond what it sees around it in detail, a person sees landmarks
//! far off in the direction it faces (water, trees, bushes, rocks, huts,
//! fires), unless a hill is in the way. What it makes out is vague, more so
//! the farther it is: it goes into the mental map as a rough place to go and
//! look, like a hint.

use super::Engine;
use crate::cognition::LandmarkKind;
use crate::policy::ExplorationHeading;
use crate::{AgentId, FeatureKind, SpawnKind, StructureKind, WorldPosition, WorldRect};

/// How far (cells) the eye reaches ahead.
pub(super) const FAR_SIGHT: i64 = 80;
/// Nearer than this (cells), near sight already sees it in detail.
const NEAR_SIGHT: i64 = 10;
/// Beyond this (cells), a bitter bush and a sweet one look the same.
const TELL_BUSHES_APART: i64 = 40;
/// Spacing (cells) of the cells checked for water.
const WATER_SAMPLE: i64 = 6;
/// Spacing (cells) of the points checked along a line of sight.
const SIGHT_STEP: i64 = 4;
/// How high the eye is, and how tall what it looks for stands (elevation units).
const EYE_HEIGHT: i64 = 256;
const TREE_HEIGHT: i64 = 512;
const LOW_HEIGHT: i64 = 64;
/// How sure it is of a place seen from afar.
const FAR_CONFIDENCE: u8 = 96;

/// Something seen far off: what it looks like, where it seems to be, and how
/// far around that to look (in 4-cell units).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Sighting {
    pub(super) kind: LandmarkKind,
    pub(super) estimate: WorldPosition,
    pub(super) uncertainty: u8,
}

impl Sighting {
    pub(super) const CONFIDENCE: u8 = FAR_CONFIDENCE;
}

impl Engine {
    /// What `agent` at `origin`, facing `facing`, makes out far ahead: the
    /// nearest of each kind of landmark in a quarter circle `FAR_SIGHT` deep.
    pub(super) fn far_sight(
        &self,
        agent: AgentId,
        origin: WorldPosition,
        facing: ExplorationHeading,
    ) -> Vec<Sighting> {
        let (hx, hy) = facing.delta();
        let in_cone = |target: WorldPosition| {
            let (dx, dy) = (target.x - origin.x, target.y - origin.y);
            let reach = dx.abs().max(dy.abs());
            if !(NEAR_SIGHT..=FAR_SIGHT).contains(&reach) {
                return false;
            }
            // Within 45 degrees of the facing: cos^2 >= 1/2.
            let dot = dx * hx + dy * hy;
            dot > 0 && 2 * dot * dot >= (dx * dx + dy * dy) * (hx * hx + hy * hy)
        };
        // The box around the quarter circle ahead.
        let span = |toward: i64, at: i64| match toward.signum() {
            1 => (at, at + FAR_SIGHT + 1),
            -1 => (at - FAR_SIGHT, at + 1),
            _ => (at - FAR_SIGHT, at + FAR_SIGHT + 1),
        };
        let ((min_x, max_x), (min_y, max_y)) = (span(hx, origin.x), span(hy, origin.y));
        let area = WorldRect {
            min: WorldPosition { x: min_x, y: min_y },
            max: WorldPosition { x: max_x, y: max_y },
        };
        let distance =
            |target: WorldPosition| (target.x - origin.x).abs().max((target.y - origin.y).abs());
        // The nearest candidate of each kind, row-major on ties.
        let mut nearest: Vec<(LandmarkKind, WorldPosition, i64)> = Vec::new();
        let mut offer = |kind: LandmarkKind, target: WorldPosition, height: i64| {
            if !in_cone(target) {
                return;
            }
            let key = (distance(target), target.y, target.x);
            let better = nearest
                .iter()
                .find(|(seen, _, _)| *seen == kind)
                .is_none_or(|&(_, other, _)| key < (distance(other), other.y, other.x));
            if better && self.in_line_of_sight(origin, target, height) {
                nearest.retain(|(seen, _, _)| *seen != kind);
                nearest.push((kind, target, height));
            }
        };
        let bush = |kind: FeatureKind, target: WorldPosition| {
            if kind == FeatureKind::BitterBush && distance(target) <= TELL_BUSHES_APART {
                LandmarkKind::BITTERBERRIES
            } else {
                LandmarkKind::BERRIES
            }
        };
        let mut features = Vec::new();
        self.world.visit_features_in_area(area, |feature| {
            if in_cone(feature.position) {
                let (kind, height) = match feature.kind {
                    FeatureKind::Tree => (LandmarkKind::WOOD, TREE_HEIGHT),
                    FeatureKind::Rock => (LandmarkKind::STONE, LOW_HEIGHT),
                    kind => (bush(kind, feature.position), LOW_HEIGHT),
                };
                features.push((
                    (
                        distance(feature.position),
                        feature.position.y,
                        feature.position.x,
                    ),
                    kind,
                    height,
                ));
            }
        });
        // Nearest first within each kind, looking further only when a hill
        // hides the nearest or it's dead.
        for kind in [
            LandmarkKind::WOOD,
            LandmarkKind::STONE,
            LandmarkKind::BERRIES,
            LandmarkKind::BITTERBERRIES,
        ] {
            let mut candidates: Vec<_> = features
                .iter()
                .filter(|(_, seen, _)| *seen == kind)
                .copied()
                .collect();
            while let Some(index) = candidates
                .iter()
                .enumerate()
                .min_by_key(|(_, (key, _, _))| *key)
                .map(|(index, _)| index)
            {
                let ((_, y, x), _, height) = candidates.swap_remove(index);
                let position = WorldPosition { x, y };
                if !self.resource_deltas.is_dead(position)
                    && self.in_line_of_sight(origin, position, height)
                {
                    offer(kind, position, height);
                    break;
                }
            }
        }
        for object in self.spawned_objects.in_area(area) {
            let looks = match object.kind {
                SpawnKind::Tree => Some((LandmarkKind::WOOD, TREE_HEIGHT)),
                SpawnKind::BerryBush => {
                    Some((bush(FeatureKind::BerryBush, object.position), LOW_HEIGHT))
                }
                SpawnKind::BitterBush => {
                    Some((bush(FeatureKind::BitterBush, object.position), LOW_HEIGHT))
                }
                SpawnKind::Rock => Some((LandmarkKind::STONE, LOW_HEIGHT)),
                SpawnKind::Water => Some((LandmarkKind::Water, 0)),
            };
            if let Some((kind, height)) = looks {
                offer(kind, object.position, height);
            }
        }
        for structure in self.structures.views(usize::MAX) {
            if structure.state != crate::StructureState::Complete {
                continue;
            }
            let kind = match structure.kind {
                StructureKind::Shelter => LandmarkKind::SHELTER,
                StructureKind::Hearth => LandmarkKind::HEARTH,
                StructureKind::Rack | StructureKind::Pit => continue,
            };
            offer(kind, structure.position, LOW_HEIGHT);
        }
        // Water: a sparse grid of the ground ahead, nearest first.
        let mut water: Vec<WorldPosition> = Vec::new();
        for y in (area.min.y..area.max.y).step_by(WATER_SAMPLE as usize) {
            for x in (area.min.x..area.max.x).step_by(WATER_SAMPLE as usize) {
                let cell = WorldPosition { x, y };
                if in_cone(cell)
                    && self
                        .world
                        .cell(cell)
                        .and_then(|terrain| terrain.water())
                        .is_some_and(|source| source.is_drinkable())
                {
                    water.push(cell);
                }
            }
        }
        water.sort_unstable_by_key(|cell| (distance(*cell), cell.y, cell.x));
        if let Some(&cell) = water
            .iter()
            .find(|&&cell| self.in_line_of_sight(origin, cell, 0))
        {
            offer(LandmarkKind::Water, cell, 0);
        }
        // Far off, it's a rough place: off by up to a tenth of the distance.
        let roll = crate::wildlife::mix(
            self.config.life_seed ^ 0x5349_4748 ^ self.time.ticks() ^ u64::from(agent.get()) << 32,
        );
        nearest.sort_unstable_by_key(|(kind, _, _)| kind.index());
        nearest
            .into_iter()
            .enumerate()
            .map(|(index, (kind, target, _))| {
                let far = distance(target);
                let spread = far / 10;
                let draw = roll.rotate_left(index as u32 * 8);
                let offset = |bits: u64| (bits % (2 * spread as u64 + 1)) as i64 - spread;
                Sighting {
                    kind,
                    estimate: WorldPosition {
                        x: target.x + offset(draw),
                        y: target.y + offset(draw >> 16),
                    },
                    uncertainty: (1 + far / 20) as u8,
                }
            })
            .collect()
    }

    /// Whether the ground between an eye at `from` and something `height`
    /// tall at `to` stays below the line between them.
    fn in_line_of_sight(&self, from: WorldPosition, to: WorldPosition, height: i64) -> bool {
        let elevation = |at: WorldPosition| {
            self.world
                .cell(at)
                .map_or(0, |cell| i64::from(cell.elevation))
        };
        let eye = elevation(from) + EYE_HEIGHT;
        let top = elevation(to) + height;
        let steps = (to.x - from.x).abs().max((to.y - from.y).abs()) / SIGHT_STEP;
        (1..steps).all(|step| {
            let at = WorldPosition {
                x: from.x + (to.x - from.x) * step / steps,
                y: from.y + (to.y - from.y) * step / steps,
            };
            elevation(at) <= eye + (top - eye) * step / steps
        })
    }
}
