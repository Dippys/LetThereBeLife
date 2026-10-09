//! Channel extraction and refinement: river sources, stream orders, segment
//! geometry, collision checks, and deterministic channel meanders.

use super::lattice::{is_border, node_position};
use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn extract_channels(
    seed: u64,
    elevation: &[i32],
    fill: &[i32],
    target: &[u32],
    accumulation: &[u64],
    basin_ids: &[u32],
    lake_ids: &[u32],
    lakes: &[LakeDescriptor],
    grid: usize,
    step: i64,
    source_flow_threshold: u64,
) -> (Vec<ChannelLink>, Vec<DrainageSegment>, Vec<RiverSource>) {
    let mut order: Vec<u32> = (0..elevation.len() as u32).collect();
    order.sort_unstable_by_key(|&index| (Reverse(fill[index as usize]), index));
    let (selected, sources) = select_lake_fed_channels(
        elevation,
        target,
        accumulation,
        basin_ids,
        lake_ids,
        lakes,
        grid,
        step,
        source_flow_threshold,
    );
    let mut channel_ids = vec![0_u32; elevation.len()];
    let mut strongest_flow = vec![0_u64; elevation.len()];
    let stream_orders = derive_stream_orders(target, &selected, &order);
    for &index in &order {
        let index = index as usize;
        if !selected[index] || target[index] == NO_TARGET {
            continue;
        }
        if channel_ids[index] == 0 {
            channel_ids[index] = index as u32 + 1;
        }
        let downstream = target[index] as usize;
        let candidate = (accumulation[index], Reverse(channel_ids[index]));
        let current = (strongest_flow[downstream], Reverse(channel_ids[downstream]));
        if candidate > current {
            strongest_flow[downstream] = accumulation[index];
            channel_ids[downstream] = channel_ids[index];
        }
    }

    let subdivisions = (step / NODE_STEP) as usize;
    let mut channels = Vec::new();
    let mut rivers = Vec::new();
    let mut rebuild_straight = false;
    for from in 0..elevation.len() {
        if !selected[from] || elevation[from] <= SEA_LEVEL || target[from] == NO_TARGET {
            continue;
        }
        let stream_order = stream_orders[from];
        let to = target[from] as usize;
        if lake_ids[from] != 0 && lake_ids[from] == lake_ids[to] {
            continue;
        }
        let outlet = if elevation[to] <= SEA_LEVEL {
            ChannelOutlet::Ocean
        } else if lake_ids[to] != 0 {
            ChannelOutlet::Lake
        } else if is_border(to, grid) {
            ChannelOutlet::WorldEdge
        } else if selected[to] && target[to] != NO_TARGET {
            if channel_ids[to] == channel_ids[from] {
                ChannelOutlet::Continues
            } else {
                ChannelOutlet::Confluence
            }
        } else {
            ChannelOutlet::TerminalBasin
        };
        let flow = accumulation[from].min(u64::from(u32::MAX)) as u32;
        let channel_id = channel_ids[from];
        debug_assert_ne!(channel_id, 0);
        debug_assert_ne!(stream_order, 0);
        channels.push(ChannelLink {
            from: from as u32,
            to: to as u32,
            flow,
            channel_id,
            basin_id: basin_ids[from],
            water_body_id: lake_ids[to],
            outlet,
            stream_order,
        });
        let half_width = channel_half_width(accumulation[from], stream_order);
        let start = node_position(from, grid, step);
        let end = node_position(to, grid, step);
        let refined = [100, 50, 0]
            .into_iter()
            .map(|strength| {
                refine_channel_link(
                    seed,
                    start,
                    end,
                    fill[from],
                    fill[to],
                    subdivisions,
                    strength,
                    channel_id,
                    half_width,
                    stream_order,
                )
            })
            .find(|candidate| !refinement_collides(candidate, &rivers, start, end))
            .unwrap_or_else(|| {
                rebuild_straight = true;
                refine_channel_link(
                    seed,
                    start,
                    end,
                    fill[from],
                    fill[to],
                    subdivisions,
                    0,
                    channel_id,
                    half_width,
                    stream_order,
                )
            });
        rivers.extend(refined);
    }
    if rebuild_straight {
        rivers.clear();
        for link in &channels {
            let from = link.from as usize;
            let to = link.to as usize;
            rivers.extend(refine_channel_link(
                seed,
                node_position(from, grid, step),
                node_position(to, grid, step),
                fill[from],
                fill[to],
                subdivisions,
                0,
                link.channel_id,
                channel_half_width(u64::from(link.flow), link.stream_order),
                link.stream_order,
            ));
        }
    }
    debug_assert!(channels.iter().all(|link| {
        sources
            .iter()
            .any(|source| source.node.saturating_add(1) == link.channel_id)
    }));
    (channels, rivers, sources)
}

fn channel_half_width(flow: u64, stream_order: u8) -> u8 {
    let local_width = 2 + u64::from(stream_order) + (flow / 260_000).isqrt();
    let major_width = 3 + (flow / 130_000).isqrt();
    local_width
        .max(if flow >= MAJOR_RIVER_FLOW_THRESHOLD {
            major_width
        } else {
            0
        })
        .min(RIVER_MAX_HALF_WIDTH as u64) as u8
}

#[allow(clippy::too_many_arguments)]
fn refine_channel_link(
    seed: u64,
    start: (i64, i64),
    end: (i64, i64),
    surface_start: i32,
    surface_end: i32,
    subdivisions: usize,
    strength: i64,
    channel_id: u32,
    half_width: u8,
    stream_order: u8,
) -> Vec<DrainageSegment> {
    let mut segments = Vec::with_capacity(subdivisions);
    let mut previous = channel_point_with_strength(seed, start, end, 0, subdivisions, strength);
    let mut previous_surface = channel_surface(surface_start, surface_end, 0, subdivisions);
    for part in 1..=subdivisions {
        let next = channel_point_with_strength(seed, start, end, part, subdivisions, strength);
        let next_surface = channel_surface(surface_start, surface_end, part, subdivisions);
        if next != previous {
            segments.push(DrainageSegment {
                ax: previous.0 as i32,
                ay: previous.1 as i32,
                bx: next.0 as i32,
                by: next.1 as i32,
                channel_id,
                surface_a: previous_surface,
                surface_b: next_surface,
                half_width,
                stream_order,
            });
        }
        previous = next;
        previous_surface = next_surface;
    }
    segments
}

fn refinement_collides(
    candidate: &[DrainageSegment],
    retained: &[DrainageSegment],
    start: (i64, i64),
    end: (i64, i64),
) -> bool {
    for offset in 0..candidate.len().saturating_sub(2) {
        let left = candidate[offset];
        if candidate[offset + 2..]
            .iter()
            .any(|&right| segments_intersect(left, right))
        {
            return true;
        }
    }
    let allowed_endpoint = |point: (i32, i32)| {
        point == (start.0 as i32, start.1 as i32) || point == (end.0 as i32, end.1 as i32)
    };
    candidate.iter().any(|&left| {
        retained.iter().any(|&right| {
            if !segments_intersect(left, right) {
                return false;
            }
            let (count, point) = shared_endpoint(left, right);
            count != 1 || !allowed_endpoint(point)
        })
    })
}

#[allow(clippy::too_many_arguments)]
fn select_lake_fed_channels(
    elevation: &[i32],
    target: &[u32],
    accumulation: &[u64],
    basin_ids: &[u32],
    lake_ids: &[u32],
    lakes: &[LakeDescriptor],
    grid: usize,
    step: i64,
    source_flow_threshold: u64,
) -> (Vec<bool>, Vec<RiverSource>) {
    let mut lake_sizes = vec![0_u16; lake_ids.len()];
    for &lake_id in lake_ids {
        if lake_id != 0 {
            lake_sizes[lake_id as usize - 1] = lake_sizes[lake_id as usize - 1].saturating_add(1);
        }
    }

    let mut candidates = lakes
        .iter()
        .filter_map(|lake| {
            if lake.terminal {
                return None;
            }
            let from = lake.outlet_node as usize;
            if accumulation[from] < source_flow_threshold {
                return None;
            }
            let to = target[from];
            debug_assert_ne!(to, NO_TARGET);
            debug_assert_eq!(lake_ids[from], lake.id);
            debug_assert_ne!(lake_ids[to as usize], lake.id);
            drains_to_open_water(to, target, elevation, grid).then_some((
                from,
                lake.id,
                lake_sizes[lake.id as usize - 1],
                accumulation[from],
                elevation[from],
                basin_ids[from],
            ))
        })
        .collect::<Vec<_>>();
    candidates.sort_unstable_by_key(|&(from, _, lake_size, flow, height, basin)| {
        (
            Reverse(lake_size),
            Reverse(flow),
            Reverse(height),
            basin,
            from,
        )
    });

    let mut selected = vec![false; target.len()];
    let mut sources = Vec::with_capacity(MAX_RIVER_SOURCES);
    for (from, lake_id, _, _, _, _) in candidates {
        if selected[from] {
            continue;
        }
        let position = node_position(from, grid, step);
        if sources.iter().any(|source: &RiverSource| {
            let other = node_position(source.node as usize, grid, step);
            let dx = position.0 - other.0;
            let dy = position.1 - other.1;
            dx * dx + dy * dy < RIVER_SOURCE_SPACING * RIVER_SOURCE_SPACING
        }) {
            continue;
        }
        let mut current = from;
        while target[current] != NO_TARGET && elevation[current] > SEA_LEVEL {
            selected[current] = true;
            current = target[current] as usize;
        }
        sources.push(RiverSource {
            node: from as u32,
            lake_id,
        });
        if sources.len() == MAX_RIVER_SOURCES {
            break;
        }
    }
    (selected, sources)
}

fn drains_to_open_water(start: u32, target: &[u32], elevation: &[i32], grid: usize) -> bool {
    let mut current = start;
    for _ in 0..target.len() {
        if current == NO_TARGET {
            return false;
        }
        let index = current as usize;
        if elevation[index] <= SEA_LEVEL || is_border(index, grid) {
            return true;
        }
        current = target[index];
    }
    false
}

pub(super) fn derive_stream_orders(target: &[u32], selected: &[bool], order: &[u32]) -> Vec<u8> {
    let mut incoming_order = vec![0_u8; target.len()];
    let mut equal_order_inputs = vec![0_u8; target.len()];
    let mut stream_orders = vec![0_u8; target.len()];
    for &index in order {
        let index = index as usize;
        if !selected[index] || target[index] == NO_TARGET {
            continue;
        }
        let stream_order = if incoming_order[index] == 0 {
            1
        } else if equal_order_inputs[index] >= 2 {
            incoming_order[index].saturating_add(1)
        } else {
            incoming_order[index]
        };
        stream_orders[index] = stream_order;
        let downstream = target[index] as usize;
        match stream_order.cmp(&incoming_order[downstream]) {
            std::cmp::Ordering::Greater => {
                incoming_order[downstream] = stream_order;
                equal_order_inputs[downstream] = 1;
            }
            std::cmp::Ordering::Equal => {
                equal_order_inputs[downstream] = equal_order_inputs[downstream].saturating_add(1);
            }
            std::cmp::Ordering::Less => {}
        }
    }
    stream_orders
}

fn channel_surface(from: i32, to: i32, part: usize, subdivisions: usize) -> u16 {
    let part = part as i64;
    let subdivisions = subdivisions as i64;
    ((i64::from(from) * (subdivisions - part) + i64::from(to) * part) / subdivisions
        - i64::from(LAKE_MIN_DEPTH))
    .clamp(0, i64::from(u16::MAX)) as u16
}

pub(super) fn segments_intersect(left: DrainageSegment, right: DrainageSegment) -> bool {
    let orient = |ax: i32, ay: i32, bx: i32, by: i32, cx: i32, cy: i32| {
        let abx = i128::from(bx) - i128::from(ax);
        let aby = i128::from(by) - i128::from(ay);
        let acx = i128::from(cx) - i128::from(ax);
        let acy = i128::from(cy) - i128::from(ay);
        abx * acy - aby * acx
    };
    let a = orient(left.ax, left.ay, left.bx, left.by, right.ax, right.ay);
    let b = orient(left.ax, left.ay, left.bx, left.by, right.bx, right.by);
    let c = orient(right.ax, right.ay, right.bx, right.by, left.ax, left.ay);
    let d = orient(right.ax, right.ay, right.bx, right.by, left.bx, left.by);
    let on_segment = |ax: i32, ay: i32, bx: i32, by: i32, px: i32, py: i32| {
        px >= ax.min(bx) && px <= ax.max(bx) && py >= ay.min(by) && py <= ay.max(by)
    };
    (a.signum() != b.signum() && c.signum() != d.signum())
        || (a == 0 && on_segment(left.ax, left.ay, left.bx, left.by, right.ax, right.ay))
        || (b == 0 && on_segment(left.ax, left.ay, left.bx, left.by, right.bx, right.by))
        || (c == 0 && on_segment(right.ax, right.ay, right.bx, right.by, left.ax, left.ay))
        || (d == 0 && on_segment(right.ax, right.ay, right.bx, right.by, left.bx, left.by))
}

pub(super) fn shared_endpoint(
    left: DrainageSegment,
    right: DrainageSegment,
) -> (usize, (i32, i32)) {
    let mut count = 0;
    let mut shared = (0, 0);
    for point in [(left.ax, left.ay), (left.bx, left.by)] {
        if point == (right.ax, right.ay) || point == (right.bx, right.by) {
            count += 1;
            shared = point;
        }
    }
    (count, shared)
}

#[cfg(test)]
pub(super) fn channel_point(
    seed: u64,
    start: (i64, i64),
    end: (i64, i64),
    part: usize,
    subdivisions: usize,
) -> (i64, i64) {
    channel_point_with_strength(seed, start, end, part, subdivisions, 100)
}

fn channel_point_with_strength(
    seed: u64,
    start: (i64, i64),
    end: (i64, i64),
    part: usize,
    subdivisions: usize,
    strength: i64,
) -> (i64, i64) {
    if part == 0 {
        return start;
    }
    if part == subdivisions {
        return end;
    }
    let remaining = (subdivisions - part) as i64;
    let part = part as i64;
    let divisor = subdivisions as i64;
    let base_x = (start.0 * remaining + end.0 * part).div_euclid(divisor);
    let base_y = (start.1 * remaining + end.1 * part).div_euclid(divisor);
    let curve_bits = crate::worldgen::noise::hash(
        seed ^ RIVER_JITTER_SEED,
        start.0.saturating_add(end.0),
        start.1.saturating_add(end.1),
    );
    let primary = (curve_bits % 97) as i64 - 48;
    let inflection = ((curve_bits >> 8) % 65) as i64 - 32;
    let shape = primary + inflection * (part * 2 - divisor) / divisor;
    let envelope = 4 * part * (divisor - part);
    let lateral = shape * envelope * strength / (divisor * divisor * 100);
    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let scale = dx.abs().max(dy.abs()).max(1);
    let detail_bits =
        crate::worldgen::noise::hash(seed ^ RIVER_JITTER_SEED.rotate_left(17), base_x, base_y);
    let jitter_x = ((detail_bits % 13) as i64 - 6) * strength / 100;
    let jitter_y = (((detail_bits >> 8) % 13) as i64 - 6) * strength / 100;
    let x = base_x - dy * lateral / scale + jitter_x;
    let y = base_y + dx * lateral / scale + jitter_y;
    (
        x.clamp(WORLD_GENERATION_BOUNDS.min.x, WORLD_GENERATION_BOUNDS.max.x),
        y.clamp(WORLD_GENERATION_BOUNDS.min.y, WORLD_GENERATION_BOUNDS.max.y),
    )
}
