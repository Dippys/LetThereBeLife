//! Unit tests for layered world generation.

use super::climate::{moisture as moisture_field, temperature as temperature_field};
use super::hydrology::RegionMap;
use super::plates::macro_sample;
use super::regions::lock_region_cache;
use super::*;
use rayon::ThreadPoolBuilder;
use rayon::prelude::*;
use std::cmp::Reverse;
use std::collections::BTreeSet;
use std::sync::{Arc, Arc as SyncArc, Barrier};

/// Overview lattice used by the structural checks: `SIDE` x `SIDE` samples
/// spaced `STEP` cells apart (a 65,536-cell-wide window).
const STEP: i64 = 256;
const SIDE: usize = 256;
const PROBE_SEED: u64 = 1;

fn sample_grid(predicate: impl Fn(i64, i64) -> bool) -> Vec<bool> {
    let mut cells = vec![false; SIDE * SIDE];
    for j in 0..SIDE {
        for i in 0..SIDE {
            cells[j * SIDE + i] = predicate(
                crate::WORLD_GENERATION_BOUNDS.min.x + i as i64 * STEP,
                crate::WORLD_GENERATION_BOUNDS.min.y + j as i64 * STEP,
            );
        }
    }
    cells
}

/// Sizes of 4-connected true components, largest first.
fn component_sizes(cells: &[bool], side: usize) -> Vec<usize> {
    component_bounds(cells, side)
        .into_iter()
        .map(|(size, _, _)| size)
        .collect()
}

/// (size, bbox width, bbox height) of 4-connected components, largest first.
fn component_bounds(cells: &[bool], side: usize) -> Vec<(usize, usize, usize)> {
    let mut visited = vec![false; cells.len()];
    let mut results = Vec::new();
    for start in 0..cells.len() {
        if !cells[start] || visited[start] {
            continue;
        }
        visited[start] = true;
        let mut stack = vec![start];
        let (mut min_x, mut max_x) = (start % side, start % side);
        let (mut min_y, mut max_y) = (start / side, start / side);
        let mut size = 0;
        while let Some(index) = stack.pop() {
            size += 1;
            let x = index % side;
            let y = index / side;
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
            let neighbors = [
                x.checked_sub(1).map(|next| y * side + next),
                (x + 1 < side).then_some(y * side + x + 1),
                y.checked_sub(1).map(|next| next * side + x),
                (y + 1 < side).then_some((y + 1) * side + x),
            ];
            for neighbor in neighbors.into_iter().flatten() {
                if cells[neighbor] && !visited[neighbor] {
                    visited[neighbor] = true;
                    stack.push(neighbor);
                }
            }
        }
        results.push((size, max_x - min_x + 1, max_y - min_y + 1));
    }
    results.sort_unstable_by_key(|entry| Reverse(entry.0));
    results
}

mod features;
mod geography;
mod hydrology;
mod regions;
