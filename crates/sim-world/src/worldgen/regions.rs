//! Bounded, build-once cache of regional hydrology maps and the batch
//! preparation entry point used before parallel chunk generation.

use std::{
    collections::BTreeSet as RegionSet,
    sync::{Arc, Mutex, MutexGuard, OnceLock},
};

use rayon::prelude::*;

use super::{
    drainage,
    hydrology::{REGION_SIZE, RegionMap},
};
use crate::{CHUNK_SIZE, ChunkLoadRequest};

const REGION_CACHE_CAPACITY: usize = 64;

type RegionKey = (u64, i64, i64);
type RegionSlot = Arc<OnceLock<Arc<RegionMap>>>;

static REGION_CACHE: OnceLock<Mutex<Vec<(RegionKey, RegionSlot)>>> = OnceLock::new();

fn region_cache() -> &'static Mutex<Vec<(RegionKey, RegionSlot)>> {
    REGION_CACHE.get_or_init(|| Mutex::new(Vec::new()))
}

pub(super) fn lock_region_cache() -> MutexGuard<'static, Vec<(RegionKey, RegionSlot)>> {
    region_cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn trim_region_cache(cache: &mut Vec<(RegionKey, RegionSlot)>) {
    while cache.len() > REGION_CACHE_CAPACITY {
        let Some(position) = cache.iter().rposition(|(_, slot)| slot.get().is_some()) else {
            // More than 64 distinct regions may briefly be building at once on
            // a large machine. Never evict an in-flight build: doing so could
            // let another worker duplicate the same expensive regional solve.
            break;
        };
        cache.remove(position);
    }
}

pub(super) fn region(seed: u64, region_x: i64, region_y: i64) -> Arc<RegionMap> {
    let key = (seed, region_x, region_y);
    let slot = {
        let mut cache = lock_region_cache();
        if let Some(position) = cache.iter().position(|(entry, _)| *entry == key) {
            let entry = cache.remove(position);
            let slot = Arc::clone(&entry.1);
            cache.insert(0, entry);
            slot
        } else {
            let slot = Arc::new(OnceLock::new());
            cache.insert(0, (key, Arc::clone(&slot)));
            trim_region_cache(&mut cache);
            slot
        }
    };
    let map = Arc::clone(slot.get_or_init(|| Arc::new(RegionMap::build(seed, region_x, region_y))));
    trim_region_cache(&mut lock_region_cache());
    map
}

/// Materializes the regional prerequisites for an ordered, bounded chunk
/// request window before dependent chunk workers enter the build-once cache.
pub(crate) fn prepare_chunk_regions(seed: u64, requests: &[ChunkLoadRequest]) {
    if requests.is_empty() {
        return;
    }
    drainage::world_drainage(seed);
    let chunks_per_region = REGION_SIZE / CHUNK_SIZE;
    let mut prepared = RegionSet::new();
    for request in requests {
        let coord = request.coord();
        let key = (
            coord.x.div_euclid(chunks_per_region),
            coord.y.div_euclid(chunks_per_region),
        );
        prepared.insert(key);
    }
    prepared
        .into_iter()
        .collect::<Vec<_>>()
        .into_par_iter()
        .for_each(|(region_x, region_y)| {
            region(seed, region_x, region_y);
        });
}
