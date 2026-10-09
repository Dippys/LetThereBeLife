//! Coarse drainage lattice passes: moisture sampling, depression fill, flow
//! routing, basin labelling, and lake identification.

use super::*;

const MOISTURE_COARSE_STEP: usize = 4;

pub(super) fn moisture_lattice(seed: u64, grid: usize, node_step: i64) -> Vec<i32> {
    let coarse_grid = (grid - 1) / MOISTURE_COARSE_STEP + 1;
    let mut coarse = vec![0_i32; coarse_grid * coarse_grid];
    coarse
        .par_iter_mut()
        .enumerate()
        .for_each(|(index, moisture_value)| {
            let coarse_x = index % coarse_grid;
            let coarse_y = index / coarse_grid;
            let x = WORLD_GENERATION_BOUNDS.min.x
                + (coarse_x * MOISTURE_COARSE_STEP) as i64 * node_step;
            let y = WORLD_GENERATION_BOUNDS.min.y
                + (coarse_y * MOISTURE_COARSE_STEP) as i64 * node_step;
            let elevation = macro_sample(seed, x, y).elevation;
            *moisture_value = moisture(seed, x, y, elevation);
        });

    let mut fine = vec![0_i32; grid * grid];
    let interpolation_step = MOISTURE_COARSE_STEP as i64;
    for y in 0..grid {
        for x in 0..grid {
            let coarse_x = (x / MOISTURE_COARSE_STEP).min(coarse_grid - 2);
            let coarse_y = (y / MOISTURE_COARSE_STEP).min(coarse_grid - 2);
            let fraction_x = x as i64 - (coarse_x * MOISTURE_COARSE_STEP) as i64;
            let fraction_y = y as i64 - (coarse_y * MOISTURE_COARSE_STEP) as i64;
            let base = coarse_y * coarse_grid + coarse_x;
            let top = i64::from(coarse[base]) * (interpolation_step - fraction_x)
                + i64::from(coarse[base + 1]) * fraction_x;
            let bottom = i64::from(coarse[base + coarse_grid]) * (interpolation_step - fraction_x)
                + i64::from(coarse[base + coarse_grid + 1]) * fraction_x;
            fine[y * grid + x] = ((top * (interpolation_step - fraction_y) + bottom * fraction_y)
                / (interpolation_step * interpolation_step))
                as i32;
        }
    }
    fine
}

pub(super) fn node_position(index: usize, grid: usize, step: i64) -> (i64, i64) {
    (
        WORLD_GENERATION_BOUNDS.min.x + (index % grid) as i64 * step,
        WORLD_GENERATION_BOUNDS.min.y + (index / grid) as i64 * step,
    )
}

fn neighbors4(index: usize, grid: usize) -> impl Iterator<Item = usize> {
    let x = index % grid;
    let y = index / grid;
    [
        (x > 0).then(|| index - 1),
        (x + 1 < grid).then(|| index + 1),
        (y > 0).then(|| index - grid),
        (y + 1 < grid).then(|| index + grid),
    ]
    .into_iter()
    .flatten()
}

fn neighbors8(index: usize, grid: usize) -> impl Iterator<Item = usize> {
    let x = (index % grid) as isize;
    let y = (index / grid) as isize;
    [
        (-1, -1),
        (0, -1),
        (1, -1),
        (-1, 0),
        (1, 0),
        (-1, 1),
        (0, 1),
        (1, 1),
    ]
    .into_iter()
    .filter_map(move |(dx, dy)| {
        let nx = x + dx;
        let ny = y + dy;
        (nx >= 0 && ny >= 0 && nx < grid as isize && ny < grid as isize)
            .then(|| ny as usize * grid + nx as usize)
    })
}

pub(super) fn is_border(index: usize, grid: usize) -> bool {
    let x = index % grid;
    let y = index / grid;
    x == 0 || y == 0 || x == grid - 1 || y == grid - 1
}

pub(super) fn priority_fill(elevation: &[i32], grid: usize) -> Vec<i32> {
    let mut fill = vec![i32::MAX; elevation.len()];
    let mut done = vec![false; elevation.len()];
    let mut heap = BinaryHeap::with_capacity(elevation.len());
    for index in 0..elevation.len() {
        if is_border(index, grid) || elevation[index] <= SEA_LEVEL {
            fill[index] = elevation[index];
            heap.push(Reverse((elevation[index], index as u32)));
        }
    }
    while let Some(Reverse((level, index))) = heap.pop() {
        let index = index as usize;
        if done[index] || level > fill[index] {
            continue;
        }
        done[index] = true;
        for neighbor in neighbors4(index, grid) {
            if done[neighbor] {
                continue;
            }
            let candidate = elevation[neighbor].max(level.saturating_add(1));
            if candidate < fill[neighbor] {
                fill[neighbor] = candidate;
                heap.push(Reverse((candidate, neighbor as u32)));
            }
        }
    }
    fill
}

pub(super) fn route_flow(
    elevation: &[i32],
    fill: &[i32],
    moisture: &[i32],
    grid: usize,
    step: i64,
) -> (Vec<u32>, Vec<u64>) {
    let mut order: Vec<u32> = (0..elevation.len() as u32).collect();
    order.sort_unstable_by_key(|&index| (Reverse(fill[index as usize]), index));
    let mut target = vec![NO_TARGET; elevation.len()];
    let mut accumulation = vec![0_u64; elevation.len()];
    let area_scale = ((step / NODE_STEP) * (step / NODE_STEP)) as u64;
    for &index in &order {
        let index = index as usize;
        if elevation[index] <= SEA_LEVEL {
            continue;
        }
        let effective_runoff =
            (i64::from(moisture[index]) - i64::from(RUNOFF_MOISTURE_FLOOR)).max(0) as u64 / 96;
        accumulation[index] = accumulation[index].saturating_add(effective_runoff * area_scale);
        let downstream = neighbors8(index, grid)
            .filter(|&neighbor| {
                fill[neighbor] < fill[index]
                    && !crosses_selected_diagonal(index, neighbor, &target, grid)
            })
            .min_by_key(|&neighbor| (fill[neighbor], neighbor));
        if let Some(downstream) = downstream {
            target[index] = downstream as u32;
            accumulation[downstream] = accumulation[downstream].saturating_add(accumulation[index]);
        }
    }
    (target, accumulation)
}

fn crosses_selected_diagonal(index: usize, neighbor: usize, target: &[u32], grid: usize) -> bool {
    let x = index % grid;
    let y = index / grid;
    let neighbor_x = neighbor % grid;
    let neighbor_y = neighbor / grid;
    if x == neighbor_x || y == neighbor_y {
        return false;
    }
    let other_a = y * grid + neighbor_x;
    let other_b = neighbor_y * grid + x;
    target[other_a] == other_b as u32 || target[other_b] == other_a as u32
}

pub(super) fn identify_basins(target: &[u32], fill: &[i32]) -> Vec<u32> {
    let mut order: Vec<u32> = (0..target.len() as u32).collect();
    order.sort_unstable_by_key(|&index| (fill[index as usize], index));
    let mut basin_ids = vec![0_u32; target.len()];
    for index in order {
        let index = index as usize;
        basin_ids[index] = if target[index] == NO_TARGET {
            index as u32 + 1
        } else {
            let downstream_id = basin_ids[target[index] as usize];
            debug_assert_ne!(downstream_id, 0);
            downstream_id
        };
    }
    basin_ids
}

pub(super) fn identify_lakes(
    elevation: &[i32],
    fill: &[i32],
    target: &[u32],
    lake_depth: &[u16],
    grid: usize,
) -> (Vec<u32>, Vec<LakeDescriptor>) {
    let mut ids = vec![0_u32; elevation.len()];
    let mut lakes = Vec::new();
    let mut stack = Vec::new();
    let mut component = Vec::new();
    for start in 0..elevation.len() {
        if ids[start] != 0
            || elevation[start] <= SEA_LEVEL
            || i32::from(lake_depth[start]) < LAKE_MIN_DEPTH
        {
            continue;
        }
        let id = start as u32 + 1;
        ids[start] = id;
        stack.push(start);
        component.clear();
        while let Some(index) = stack.pop() {
            component.push(index);
            for neighbor in neighbors4(index, grid) {
                if ids[neighbor] == 0
                    && elevation[neighbor] > SEA_LEVEL
                    && i32::from(lake_depth[neighbor]) >= LAKE_MIN_DEPTH
                {
                    ids[neighbor] = id;
                    stack.push(neighbor);
                }
            }
        }
        let outlet = component
            .iter()
            .filter_map(|&index| {
                let downstream = target[index];
                (downstream != NO_TARGET && ids[downstream as usize] != id).then_some((
                    fill[downstream as usize],
                    index,
                    downstream as usize,
                ))
            })
            .min();
        let (outlet_node, terminal, spill_elevation) = outlet.map_or_else(
            || (start, true, fill[start]),
            |(_, from, downstream)| (from, false, fill[downstream]),
        );
        lakes.push(LakeDescriptor {
            id,
            outlet_node: outlet_node as u32,
            spill_elevation: spill_elevation.clamp(0, u16::MAX as i32) as u16,
            terminal,
        });
    }
    (ids, lakes)
}
