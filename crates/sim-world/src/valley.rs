//! Site selection for small social scenarios: find a livable "valley" (mostly
//! walkable land with some fresh water and food) near the origin by sampling the
//! generator, without materializing any terrain.

use crate::{
    BiomeType, CHUNK_SIZE, ChunkCoord, ChunkGenerator, ChunkLocalPosition, FeatureKind,
    Standability, SurfaceType, WORLD_GENERATION_BOUNDS, World, WorldPosition, WorldRect,
};

/// Distance between candidate valley centers.
const CANDIDATE_SPACING: i64 = 1_024;
/// Candidates per axis (centered on the origin): 7 × 7 = 49 sites.
const CANDIDATES_PER_AXIS: i64 = 7;
/// Sampling stride inside a candidate square.
const SAMPLE_STEP: i64 = 8;
/// Fresh-water samples needed: a few ponds or one river reach.
const MIN_FRESH_SAMPLES: u32 = 8;

/// How livable a sampled square is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValleyScore {
    /// Percent of samples that are walkable dry land (not water, rock, snow, or hill).
    pub land_percent: u32,
    /// Percent of samples that are fresh water (lake or river).
    pub fresh_water_percent: u32,
    /// Samples that are fresh water.
    pub fresh_water_samples: u32,
    /// Percent of samples that are ocean.
    pub ocean_percent: u32,
    /// Samples holding a berry bush.
    pub food_samples: u32,
    /// Samples holding a tree.
    pub wood_samples: u32,
}

impl ValleyScore {
    /// Livable: mostly land, some fresh water without being a lake, no sea, some food.
    pub const fn is_livable(self) -> bool {
        self.land_percent >= 70
            && self.fresh_water_samples >= MIN_FRESH_SAMPLES
            && self.fresh_water_percent <= 20
            && self.ocean_percent == 0
            && self.food_samples >= 8
    }

    /// Higher is better among livable sites: food first, then wood, then water.
    pub fn rank(self) -> u32 {
        self.food_samples * 4 + self.wood_samples + self.fresh_water_samples.min(400)
    }
}

/// A chosen valley: its bounds and why it was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Valley {
    pub bounds: WorldRect,
    pub score: ValleyScore,
}

/// The best livable `side × side` square among candidates around the origin,
/// or `None` if no candidate qualifies. `side` is rounded up to whole chunks.
/// Deterministic for a seed: equal inputs always pick the same valley.
pub fn find_valley(seed: u64, side: i64) -> Option<Valley> {
    candidates(seed, side)
        .into_iter()
        .filter_map(|(grid, valley)| Some((grid, valley?)))
        // Prefer better sites, then sites closer to the origin, then the first scanned.
        .min_by_key(|((gx, gy), valley)| {
            (std::cmp::Reverse(valley.score.rank()), gx.abs() + gy.abs())
        })
        .map(|(_, valley)| valley)
}

/// Two livable `side × side` squares in neighboring candidate spots (their
/// centers `CANDIDATE_SPACING` apart), for groups that start out of each
/// other's reach. The best pair by the worse of the two, then closest to the origin.
pub fn find_valley_pair(seed: u64, side: i64) -> Option<[Valley; 2]> {
    let candidates = candidates(seed, side);
    let at = |gx: i64, gy: i64| {
        candidates
            .iter()
            .find(|(grid, _)| *grid == (gx, gy))
            .and_then(|(_, valley)| *valley)
    };
    candidates
        .iter()
        .filter_map(|&((gx, gy), valley)| Some(((gx, gy), valley?)))
        .flat_map(|((gx, gy), first)| {
            [(gx + 1, gy), (gx, gy + 1)]
                .into_iter()
                .filter_map(move |(nx, ny)| Some(((gx, gy), first, at(nx, ny)?)))
                .collect::<Vec<_>>()
        })
        .min_by_key(|((gx, gy), first, second)| {
            (
                std::cmp::Reverse(first.score.rank().min(second.score.rank())),
                gx.abs() + gy.abs(),
            )
        })
        .map(|(_, first, second)| [first, second])
}

/// Every candidate spot around the origin (grid coordinates), with its square
/// if it's inside the world and livable. `side` is rounded up to whole chunks.
fn candidates(seed: u64, side: i64) -> Vec<((i64, i64), Option<Valley>)> {
    let side = (side.max(CHUNK_SIZE) + CHUNK_SIZE - 1) / CHUNK_SIZE * CHUNK_SIZE;
    let half = CANDIDATES_PER_AXIS / 2;
    let mut found = Vec::new();
    for gy in -half..=half {
        for gx in -half..=half {
            let center = WorldPosition {
                x: gx * CANDIDATE_SPACING,
                y: gy * CANDIDATE_SPACING,
            };
            let min = WorldPosition {
                x: (center.x - side / 2).div_euclid(CHUNK_SIZE) * CHUNK_SIZE,
                y: (center.y - side / 2).div_euclid(CHUNK_SIZE) * CHUNK_SIZE,
            };
            let bounds = WorldRect {
                min,
                max: WorldPosition {
                    x: min.x + side,
                    y: min.y + side,
                },
            };
            let valley = WORLD_GENERATION_BOUNDS
                .contains_rect(bounds)
                .then(|| score_square(seed, bounds))
                .filter(|score| score.is_livable())
                .map(|score| Valley { bounds, score });
            found.push(((gx, gy), valley));
        }
    }
    found
}

/// Samples a square every `SAMPLE_STEP` cells.
pub fn score_square(seed: u64, bounds: WorldRect) -> ValleyScore {
    let (mut total, mut land, mut fresh, mut ocean, mut food, mut wood) = (0, 0, 0, 0, 0, 0);
    let mut chunk_y = bounds.min.y.div_euclid(CHUNK_SIZE);
    while chunk_y * CHUNK_SIZE < bounds.max.y {
        let mut chunk_x = bounds.min.x.div_euclid(CHUNK_SIZE);
        while chunk_x * CHUNK_SIZE < bounds.max.x {
            let coord = ChunkCoord {
                x: chunk_x,
                y: chunk_y,
            };
            if let Ok(generator) = ChunkGenerator::new(seed, coord) {
                for local_y in (SAMPLE_STEP / 2..CHUNK_SIZE).step_by(SAMPLE_STEP as usize) {
                    for local_x in (SAMPLE_STEP / 2..CHUNK_SIZE).step_by(SAMPLE_STEP as usize) {
                        let Some(cell) = generator.sample(ChunkLocalPosition {
                            x: local_x as u8,
                            y: local_y as u8,
                        }) else {
                            continue;
                        };
                        total += 1;
                        match (cell.terrain.biome(), cell.terrain.surface()) {
                            (BiomeType::Ocean, _) => ocean += 1,
                            (BiomeType::Lake | BiomeType::River, _) => fresh += 1,
                            (_, SurfaceType::Sand | SurfaceType::Soil) => land += 1,
                            _ => {}
                        }
                        match cell.feature {
                            Some(FeatureKind::BerryBush) => food += 1,
                            Some(FeatureKind::Tree) => wood += 1,
                            _ => {}
                        }
                    }
                }
            }
            chunk_x += 1;
        }
        chunk_y += 1;
    }
    let percent = |part: u32| part * 100 / total.max(1);
    ValleyScore {
        land_percent: percent(land),
        fresh_water_percent: percent(fresh),
        fresh_water_samples: fresh,
        ocean_percent: percent(ocean),
        food_samples: food,
        wood_samples: wood,
    }
}

/// Side of the default valley square in cells.
pub const VALLEY_SIDE: i64 = 768;
/// The spec's first vertical slice: a band of 16 adults.
pub const VALLEY_BAND: usize = 16;

/// Band members start within this many cells of the camp's water access.
pub const CAMP_RADIUS: i64 = 8;

/// Families in the valley band. Each camps at its own water source.
pub const VALLEY_FAMILIES: usize = 2;
/// Children per family (spec: 16 adults and 4 children).
pub const VALLEY_CHILDREN_PER_FAMILY: usize = 2;

/// Where a band with children starts. Founders come first in family order
/// (ids `0..founders`), then children in family order; `parents` pairs each
/// child's index with a founder of its own family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BandLayout {
    pub sites: Vec<WorldPosition>,
    pub founders: usize,
    pub parents: Vec<(usize, usize)>,
}

/// Lays out `families` families of `adults` founders and `children` children,
/// each family around its own camp (see [`family_camps`]).
pub fn band_layout(
    world: &World,
    bounds: WorldRect,
    families: usize,
    adults: usize,
    children: usize,
    seed: u64,
) -> Option<BandLayout> {
    let size = adults + children;
    if adults == 0 {
        return None;
    }
    let camps = family_camps(world, bounds, families, size, seed)?;
    Some(layout_from_camps(&camps, families, adults, children))
}

/// Like [`band_layout`], but each family camps in its own valley
/// (`valleys[family]`), out of reach of the others.
pub fn apart_layout(
    world: &World,
    valleys: &[WorldRect],
    adults: usize,
    children: usize,
    seed: u64,
) -> Option<BandLayout> {
    let size = adults + children;
    if adults == 0 {
        return None;
    }
    let mut camps = Vec::with_capacity(valleys.len() * size);
    for &valley in valleys {
        camps.extend(family_camps(world, valley, 1, size, seed)?);
    }
    Some(layout_from_camps(&camps, valleys.len(), adults, children))
}

/// Founders first in family order, then children, from per-family camp sites.
fn layout_from_camps(
    camps: &[WorldPosition],
    families: usize,
    adults: usize,
    children: usize,
) -> BandLayout {
    let size = adults + children;
    let mut sites: Vec<_> = (0..families)
        .flat_map(|family| camps[family * size..family * size + adults].iter().copied())
        .collect();
    let mut parents = Vec::with_capacity(families * children);
    for family in 0..families {
        for child in 0..children {
            parents.push((sites.len(), family * adults + child % adults));
            sites.push(camps[family * size + adults + child]);
        }
    }
    BandLayout {
        sites,
        founders: families * adults,
        parents,
    }
}

/// Standable cells next to drinkable water inside `bounds`, row-major.
fn water_accesses(world: &World, bounds: WorldRect) -> Vec<WorldPosition> {
    let standable = |position: WorldPosition| {
        bounds.contains(position) && world.standability_at(position) == Ok(Standability::Standable)
    };
    let mut accesses: Vec<_> = world
        .cells()
        .map(|(position, _)| position)
        .filter(|position| {
            bounds.contains(*position)
                && world
                    .water_at(*position)
                    .is_ok_and(|source| source.is_some_and(crate::WaterSource::is_drinkable))
        })
        .flat_map(|water| {
            [(0, -1), (-1, 0), (1, 0), (0, 1)].map(|(dx, dy)| WorldPosition {
                x: water.x + dx,
                y: water.y + dy,
            })
        })
        .filter(|cell| standable(*cell))
        .collect();
    accesses.sort_unstable_by_key(|cell| (cell.y, cell.x));
    accesses.dedup();
    accesses
}

/// Up to `count` distinct standable cells within `CAMP_RADIUS` of `camp`, the camp first.
fn members_around(
    world: &World,
    bounds: WorldRect,
    camp: WorldPosition,
    count: usize,
    seed: u64,
    taken: &[WorldPosition],
) -> Vec<WorldPosition> {
    let standable = |position: WorldPosition| {
        bounds.contains(position) && world.standability_at(position) == Ok(Standability::Standable)
    };
    let mut chosen = Vec::with_capacity(count);
    if !taken.contains(&camp) {
        chosen.push(camp);
    }
    let span = (CAMP_RADIUS * 2 + 1) as u64;
    for attempt in 0..100_000_u64 {
        if chosen.len() >= count {
            break;
        }
        let mut key = seed ^ 0xca4d_u64.wrapping_mul(attempt + 1) ^ (camp.x as u64).rotate_left(21);
        key = (key ^ (key >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        key = (key ^ (key >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        key ^= key >> 31;
        let cell = WorldPosition {
            x: camp.x + (key % span) as i64 - CAMP_RADIUS,
            y: camp.y + ((key >> 32) % span) as i64 - CAMP_RADIUS,
        };
        if standable(cell) && !chosen.contains(&cell) && !taken.contains(&cell) {
            chosen.push(cell);
        }
    }
    chosen
}

/// Where a band of `count` starts in a resident valley: the water access closest
/// to its center, then distinct standable cells around it. Deterministic per seed.
/// `None` if the valley has no reachable fresh water or too little room.
pub fn camp_sites(
    world: &World,
    bounds: WorldRect,
    count: usize,
    seed: u64,
) -> Option<Vec<WorldPosition>> {
    family_camps(world, bounds, 1, count, seed)
}

/// Where a band of `families × family_size` starts: the first family at the water
/// access closest to the valley's center, each later family at the access farthest
/// from the camps already chosen, members around their family's camp. Returned in
/// family order, so agent ids `0..family_size` are the first family, and so on.
pub fn family_camps(
    world: &World,
    bounds: WorldRect,
    families: usize,
    family_size: usize,
    seed: u64,
) -> Option<Vec<WorldPosition>> {
    let center = WorldPosition {
        x: (bounds.min.x + bounds.max.x) / 2,
        y: (bounds.min.y + bounds.max.y) / 2,
    };
    let accesses = water_accesses(world, bounds);
    let distance = |a: WorldPosition, b: WorldPosition| a.x.abs_diff(b.x) + a.y.abs_diff(b.y);
    let mut camps = vec![
        *accesses
            .iter()
            .min_by_key(|cell| (distance(**cell, center), cell.y, cell.x))?,
    ];
    while camps.len() < families {
        let next = *accesses.iter().max_by_key(|cell| {
            let nearest_camp = camps
                .iter()
                .map(|camp| distance(**cell, *camp))
                .min()
                .unwrap_or(0);
            (nearest_camp, std::cmp::Reverse((cell.y, cell.x)))
        })?;
        camps.push(next);
    }
    let mut sites = Vec::with_capacity(families * family_size);
    for camp in camps {
        let members = members_around(world, bounds, camp, family_size, seed, &sites);
        if members.len() < family_size {
            return None;
        }
        sites.extend(members);
    }
    Some(sites)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valleys_are_deterministic_livable_and_chunk_aligned() {
        let valley = find_valley(1, 768).expect("seed 1 has a livable valley");
        assert_eq!(find_valley(1, 768), Some(valley));
        assert!(valley.score.is_livable());
        assert_eq!(valley.bounds.min.x.rem_euclid(CHUNK_SIZE), 0);
        assert_eq!(valley.bounds.max.x - valley.bounds.min.x, 768);
        assert_eq!(score_square(1, valley.bounds), valley.score);
    }

    #[test]
    fn some_seeds_have_two_neighboring_valleys() {
        let found = (1..=20)
            .filter_map(|seed| Some((seed, find_valley_pair(seed, VALLEY_SIDE)?)))
            .collect::<Vec<_>>();
        assert!(!found.is_empty(), "no seed among 1-20 has a valley pair");
        for (seed, [first, second]) in found {
            assert!(first.score.is_livable() && second.score.is_livable());
            let gap = first.bounds.min.x.abs_diff(second.bounds.min.x)
                + first.bounds.min.y.abs_diff(second.bounds.min.y);
            assert_eq!(gap, CANDIDATE_SPACING as u64, "seed {seed}: neighbors");
        }
    }

    #[test]
    fn families_camp_apart_at_their_own_water() {
        let valley = find_valley(1, VALLEY_SIDE).expect("seed 1 has a valley");
        let mut world = World::new(1, crate::WorldConfig::new(64, 64).unwrap());
        world.generate_area(valley.bounds).unwrap();
        let sites = family_camps(&world, valley.bounds, VALLEY_FAMILIES, 8, 1).expect("room");
        assert_eq!(sites.len(), VALLEY_FAMILIES * 8);
        let (first, second) = (sites[0], sites[8]);
        assert!(
            first.x.abs_diff(second.x) + first.y.abs_diff(second.y) > 4 * CAMP_RADIUS as u64,
            "the families start apart: {first:?} vs {second:?}"
        );
        let mut unique = sites.clone();
        unique.sort_unstable_by_key(|cell| (cell.y, cell.x));
        unique.dedup();
        assert_eq!(unique.len(), sites.len());
    }

    #[test]
    fn children_follow_the_founders_and_belong_to_their_own_family() {
        let valley = find_valley(1, VALLEY_SIDE).expect("seed 1 has a valley");
        let mut world = World::new(1, crate::WorldConfig::new(64, 64).unwrap());
        world.generate_area(valley.bounds).unwrap();
        let layout = band_layout(&world, valley.bounds, 2, 8, 2, 1).expect("room");
        assert_eq!(layout.founders, 16);
        assert_eq!(layout.sites.len(), 20);
        assert_eq!(layout.parents, vec![(16, 0), (17, 1), (18, 8), (19, 9)]);
        for &(child, parent) in &layout.parents {
            let (a, b) = (layout.sites[child], layout.sites[parent]);
            assert!(a.x.abs_diff(b.x) + a.y.abs_diff(b.y) <= 4 * CAMP_RADIUS as u64);
        }
    }

    #[test]
    fn the_band_camps_together_beside_water_inside_the_valley() {
        let valley = find_valley(1, VALLEY_SIDE).expect("seed 1 has a valley");
        let mut world = World::new(1, crate::WorldConfig::new(64, 64).unwrap());
        world.generate_area(valley.bounds).unwrap();
        let sites = camp_sites(&world, valley.bounds, VALLEY_BAND, 1).expect("room to camp");
        assert_eq!(sites.len(), VALLEY_BAND);
        assert_eq!(
            camp_sites(&world, valley.bounds, VALLEY_BAND, 1),
            Some(sites.clone())
        );
        let camp = sites[0];
        assert!(
            [(0, -1), (-1, 0), (1, 0), (0, 1)]
                .iter()
                .any(|(dx, dy)| world
                    .water_at(WorldPosition {
                        x: camp.x + dx,
                        y: camp.y + dy
                    })
                    .is_ok_and(|source| source.is_some_and(crate::WaterSource::is_drinkable))),
            "the first site is a water access"
        );
        for (index, site) in sites.iter().enumerate() {
            assert!(valley.bounds.contains(*site));
            assert_eq!(world.standability_at(*site), Ok(Standability::Standable));
            assert!(site.x.abs_diff(camp.x) as i64 <= CAMP_RADIUS);
            assert!(!sites[..index].contains(site), "sites are distinct");
        }
    }
}
