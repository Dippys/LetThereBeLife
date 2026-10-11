//! Plants live, spread and die (D-111): once a year, at the start of spring,
//! old bushes may die and every living plant may seed a new one nearby, in
//! the same kind of ground, where it's less crowded than where its parent
//! grew (so thickets refill and sparse ground stays sparse). Felled trees are
//! gone (see `ResourceDeltas::gather`); seedlings start small and grow.

use super::Engine;
use crate::{FeatureKind, SpawnKind, WorldPosition};

/// A bush dies in a given year with odds 1 in this (about this many years' life).
const BUSH_LIFE_YEARS: u64 = 8;
/// A tree dies of age in a given year with odds 1 in this.
const TREE_LIFE_YEARS: u64 = 60;
/// A living bush seeds a new one in a given year with odds 1 in this.
const BUSH_SEEDING_ODDS: u64 = 2;
/// A living tree seeds a sapling in a given year with odds 1 in this.
const TREE_SEEDING_ODDS: u64 = 15;
/// How far seeds fall from the plant, in cells.
const SEED_REACH: i64 = 5;
/// How far around a plant counts as its patch (7x7 cells).
const PATCH_REACH: i64 = 3;
/// Wood a sapling starts with (it grows like any tree).
const SAPLING_WOOD: u16 = 2;

impl Engine {
    /// Once a year, at the turn of spring: plants die and seed.
    pub(super) fn grow_plants(&mut self) {
        let now = self.time.ticks();
        if now == 0 || now % crate::TICKS_PER_YEAR != 0 || self.resource_deltas.barren() {
            return;
        }
        let year = now / crate::TICKS_PER_YEAR;
        let seed = self.config.seed;
        let roll = |position: WorldPosition, salt: u64| {
            crate::wildlife::mix(
                seed ^ salt
                    ^ year.wrapping_mul(0x9e37_79b9_7f4a_7c15)
                    ^ ((position.x as u64) << 32)
                    ^ (position.y as u64 & 0xffff_ffff),
            )
        };
        let mut plants: Vec<(WorldPosition, FeatureKind, bool)> = self
            .world
            .all_features()
            .filter(|feature| feature.kind != FeatureKind::Rock)
            .filter(|feature| !self.resource_deltas.is_dead(feature.position))
            .map(|feature| (feature.position, feature.kind, false))
            .collect();
        plants.extend(self.spawned_objects.views().filter_map(|object| {
            let feature = object
                .kind
                .feature()
                .filter(|kind| *kind != FeatureKind::Rock)?;
            Some((object.position, feature, true))
        }));
        plants.sort_unstable_by_key(|&(position, kind, placed)| {
            (position.y, position.x, kind as u8, placed)
        });
        let mut living = Vec::with_capacity(plants.len());
        for (position, kind, placed) in plants {
            let life = if kind == FeatureKind::Tree {
                TREE_LIFE_YEARS
            } else {
                BUSH_LIFE_YEARS
            };
            if roll(position, 0x4445_4144) % life == 0 {
                if placed {
                    self.spawned_objects.remove(position);
                } else {
                    self.resource_deltas.kill(position);
                }
                self.plants_died += 1;
            } else {
                living.push((position, kind));
            }
        }
        for &(parent, kind) in &living {
            let odds = if kind == FeatureKind::Tree {
                TREE_SEEDING_ODDS
            } else {
                BUSH_SEEDING_ODDS
            };
            let draw = roll(parent, 0x5345_4544);
            if draw % odds != 0 {
                continue;
            }
            let span = (2 * SEED_REACH + 1) as u64;
            let spot = WorldPosition {
                x: parent.x + ((draw >> 8) % span) as i64 - SEED_REACH,
                y: parent.y + ((draw >> 24) % span) as i64 - SEED_REACH,
            };
            let same_ground = |a: WorldPosition, b: WorldPosition| {
                self.world
                    .cell(a)
                    .zip(self.world.cell(b))
                    .is_some_and(|(a, b)| a.biome() == b.biome())
            };
            if !self.can_sprout_at(spot)
                || !same_ground(parent, spot)
                || self.neighbours(spot) >= self.neighbours(parent)
            {
                continue;
            }
            let Some(sprout) = SpawnKind::growing(kind) else {
                continue;
            };
            let start = if sprout == SpawnKind::Tree {
                SAPLING_WOOD
            } else {
                0
            };
            self.spawned_objects.plant(spot, sprout, start);
            self.plants_sprouted += 1;
        }
    }

    /// Living plants in the patch around `position`.
    fn neighbours(&self, position: WorldPosition) -> usize {
        let patch = crate::WorldRect::from_inclusive_points(
            WorldPosition {
                x: position.x - PATCH_REACH,
                y: position.y - PATCH_REACH,
            },
            WorldPosition {
                x: position.x + PATCH_REACH,
                y: position.y + PATCH_REACH,
            },
        );
        let generated = self
            .world
            .features_in(patch)
            .filter(|feature| !self.resource_deltas.is_dead(feature.position))
            .count();
        let placed = (-PATCH_REACH..=PATCH_REACH)
            .flat_map(|dy| (-PATCH_REACH..=PATCH_REACH).map(move |dx| (dx, dy)))
            .filter(|&(dx, dy)| {
                self.spawned_objects
                    .at(WorldPosition {
                        x: position.x + dx,
                        y: position.y + dy,
                    })
                    .is_some()
            })
            .count();
        generated + placed
    }

    /// Plants that took root, and that died of age, since the start.
    pub fn plant_counts(&self) -> (u64, u64) {
        (self.plants_sprouted, self.plants_died)
    }

    /// Living trees and bushes (of either kind) now.
    pub fn plant_census(&self) -> (u64, u64) {
        let mut census = (0, 0);
        let mut count = |kind: FeatureKind| match kind {
            FeatureKind::Tree => census.0 += 1,
            FeatureKind::BerryBush | FeatureKind::BitterBush => census.1 += 1,
            FeatureKind::Rock => {}
        };
        for feature in self.world.all_features() {
            if !self.resource_deltas.is_dead(feature.position) {
                count(feature.kind);
            }
        }
        for object in self.spawned_objects.views() {
            if let Some(kind) = object.kind.feature() {
                count(kind);
            }
        }
        census
    }

    /// Generated plants that have died or been felled (the viewer covers them).
    pub fn dead_plants(&self) -> impl Iterator<Item = WorldPosition> + '_ {
        self.resource_deltas.dead()
    }
}
