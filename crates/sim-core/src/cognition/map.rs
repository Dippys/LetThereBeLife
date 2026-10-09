//! The per-agent mental map: a fixed number of remembered places per kind,
//! a ring of recently explored tiles, and sharing bookkeeping. Everything here
//! is the agent's private belief, never authoritative world state.

use crate::{
    PhysicalPerception, ResourceKind, WorldPosition, WorldRect, policy::ExplorationHeading,
    structures::StructureState,
};

use super::{LandmarkKind, LandmarkSource, LandmarkView};

/// Remembered-place slots per agent, partitioned by kind (see `slot_range`).
pub const LANDMARK_SLOTS: usize = 12;
/// Recently explored tiles remembered for novelty-seeking exploration.
pub const VISITED_TILE_SLOTS: usize = 24;
/// Edge length of an exploration tile in cells.
pub const VISIT_TILE_SIZE: i64 = 32;
/// Same-kind places closer than this (Chebyshev cells) merge into one memory.
pub const MERGE_RADIUS: u64 = 24;
/// How far ahead (cells) exploration looks when judging whether a direction is new.
const NOVELTY_LOOKAHEAD: i64 = 48;
/// Confidence of a first-hand observation.
const SEEN_CONFIDENCE: u8 = 255;
/// Confidence lost each time a searched hint turns up nothing.
const FAILED_PROBE_PENALTY: u8 = 40;
/// Hints below this confidence, or searched this many times, are forgotten.
const FORGET_CONFIDENCE: u8 = 24;
const MAX_PROBES: u8 = 6;
/// A food sighting loses one point of belief per this many seconds of age.
const FOOD_STALENESS_SECONDS: u64 = 20;
/// Spacing between spiral-search loops: slightly less than the view width so
/// consecutive loops overlap and nothing is missed.
pub const SEARCH_SPACING: i64 = 14;
/// Decisions spent on one spiral leg before giving up on reaching its corner
/// (for example when the corner is across water).
const SEARCH_LEG_DECISIONS: u8 = 24;
/// Marks "no spiral search in progress".
const NO_SEARCH: u16 = u16::MAX;

/// One remembered place. Its kind is implied by its slot; `confidence == 0`
/// marks an empty slot. Exactly 12 bytes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct Landmark {
    x: i16,
    y: i16,
    /// Simulation seconds (ticks / 60) when last confirmed or heard.
    seen: u32,
    confidence: u8,
    /// Search radius in 4-cell units; zero for first-hand observations.
    uncertainty: u8,
    probes: u8,
    /// For hints: acquaintance slot + 1 of whoever pointed it out (0 = unknown).
    teller: u8,
}

impl Landmark {
    fn position(self) -> WorldPosition {
        WorldPosition {
            x: i64::from(self.x),
            y: i64::from(self.y),
        }
    }

    const fn is_empty(self) -> bool {
        self.confidence == 0
    }

    const fn is_first_hand(self) -> bool {
        self.uncertainty == 0
    }

    fn radius(self) -> u64 {
        u64::from(self.uncertainty) * 4
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct MentalMap {
    landmarks: [Landmark; LANDMARK_SLOTS],
    visited: [(i16, i16); VISITED_TILE_SLOTS],
    visited_len: u8,
    visited_cursor: u8,
    share_cursor: u8,
    search_leg_decisions: u8,
    /// Simulation seconds of the last gesture this agent made; `u32::MAX` = never.
    last_share: u32,
    /// Center of the current spiral search for water.
    search_anchor: (i16, i16),
    /// Index of the spiral corner currently sought; `NO_SEARCH` when idle.
    search_step: u16,
}

impl Default for MentalMap {
    fn default() -> Self {
        Self {
            landmarks: [Landmark::default(); LANDMARK_SLOTS],
            visited: [(0, 0); VISITED_TILE_SLOTS],
            visited_len: 0,
            visited_cursor: 0,
            share_cursor: 0,
            search_leg_decisions: 0,
            last_share: u32::MAX,
            search_anchor: (0, 0),
            search_step: NO_SEARCH,
        }
    }
}

const fn slot_range(kind: LandmarkKind) -> std::ops::Range<usize> {
    match kind {
        LandmarkKind::Water => 0..4,
        LandmarkKind::Food => 4..7,
        LandmarkKind::Wood => 7..9,
        LandmarkKind::Stone => 9..10,
        LandmarkKind::Shelter => 10..12,
    }
}

const fn kind_of_slot(slot: usize) -> LandmarkKind {
    match slot {
        0..4 => LandmarkKind::Water,
        4..7 => LandmarkKind::Food,
        7..9 => LandmarkKind::Wood,
        9..10 => LandmarkKind::Stone,
        _ => LandmarkKind::Shelter,
    }
}

fn chebyshev(left: WorldPosition, right: WorldPosition) -> u64 {
    left.x.abs_diff(right.x).max(left.y.abs_diff(right.y))
}

fn manhattan(left: WorldPosition, right: WorldPosition) -> u64 {
    left.x.abs_diff(right.x) + left.y.abs_diff(right.y)
}

fn contains(area: WorldRect, position: WorldPosition) -> bool {
    position.x >= area.min.x
        && position.x < area.max.x
        && position.y >= area.min.y
        && position.y < area.max.y
}

fn tile_of(position: WorldPosition) -> (i16, i16) {
    (
        position.x.div_euclid(VISIT_TILE_SIZE) as i16,
        position.y.div_euclid(VISIT_TILE_SIZE) as i16,
    )
}

/// Nearest perceived instance of each landmark kind, in kind order.
fn perceived_nearest(
    origin: WorldPosition,
    perception: &PhysicalPerception,
) -> [Option<WorldPosition>; 5] {
    let nearest = |positions: &mut dyn Iterator<Item = WorldPosition>| {
        positions.min_by_key(|position| (manhattan(origin, *position), position.y, position.x))
    };
    let resource = |kind: ResourceKind| {
        nearest(
            &mut perception
                .resources
                .iter()
                .filter(move |resource| resource.resource.kind == kind)
                .map(|resource| resource.position),
        )
    };
    [
        nearest(
            &mut perception
                .drinkable_water
                .iter()
                .map(|water| water.position),
        ),
        resource(ResourceKind::Food),
        resource(ResourceKind::Wood),
        resource(ResourceKind::Stone),
        nearest(
            &mut perception
                .structures
                .iter()
                .filter(|structure| structure.state == StructureState::Complete)
                .map(|structure| structure.position),
        ),
    ]
}

impl MentalMap {
    /// Updates beliefs from one perception: forgets places that turned out empty,
    /// remembers the nearest visible place of each kind, and marks the tile explored.
    /// `on_hint(teller_slot, confirmed, kind)` reports hints that were confirmed by
    /// seeing the place or abandoned after failed searches.
    pub(crate) fn observe(
        &mut self,
        agent: u32,
        origin: WorldPosition,
        perception: &PhysicalPerception,
        now: u32,
        on_hint: &mut impl FnMut(u8, bool, LandmarkKind),
    ) {
        let _ = self.record_visit(origin);
        let nearest = perceived_nearest(origin, perception);
        for slot in 0..LANDMARK_SLOTS {
            let landmark = self.landmarks[slot];
            if landmark.is_empty() || nearest[kind_of_slot(slot) as usize].is_some() {
                continue;
            }
            let checked = if landmark.is_first_hand() {
                landmark.position()
            } else {
                probe_point(landmark, agent, slot)
            };
            if !contains(perception.area, checked) {
                continue;
            }
            // Looked right at it and saw nothing of that kind anywhere in view.
            let slot_ref = &mut self.landmarks[slot];
            if slot_ref.is_first_hand() {
                *slot_ref = Landmark::default();
            } else {
                slot_ref.probes = slot_ref.probes.saturating_add(1);
                slot_ref.confidence = slot_ref.confidence.saturating_sub(FAILED_PROBE_PENALTY);
                if slot_ref.confidence < FORGET_CONFIDENCE || slot_ref.probes >= MAX_PROBES {
                    if slot_ref.teller != 0 {
                        on_hint(slot_ref.teller - 1, false, kind_of_slot(slot));
                    }
                    *slot_ref = Landmark::default();
                }
            }
        }
        for kind in LandmarkKind::ALL {
            if let Some(position) = nearest[kind as usize] {
                self.remember_seen(kind, position, now, on_hint);
            }
        }
    }

    fn remember_seen(
        &mut self,
        kind: LandmarkKind,
        position: WorldPosition,
        now: u32,
        on_hint: &mut impl FnMut(u8, bool, LandmarkKind),
    ) {
        let Some((x, y)) = compact(position) else {
            return;
        };
        let range = slot_range(kind);
        for slot in range.clone() {
            let landmark = &mut self.landmarks[slot];
            if landmark.is_empty() {
                continue;
            }
            if landmark.is_first_hand() && chebyshev(landmark.position(), position) <= MERGE_RADIUS
            {
                landmark.seen = now;
                landmark.confidence = SEEN_CONFIDENCE;
                return;
            }
            // A hint that this sighting explains is now confirmed and replaced below.
            if !landmark.is_first_hand()
                && chebyshev(landmark.position(), position) <= landmark.radius() + MERGE_RADIUS
            {
                if landmark.teller != 0 {
                    on_hint(landmark.teller - 1, true, kind);
                }
                *landmark = Landmark::default();
            }
        }
        let slot = self.replacement_slot(range, true);
        if let Some(slot) = slot {
            self.landmarks[slot] = Landmark {
                x,
                y,
                seen: now,
                confidence: SEEN_CONFIDENCE,
                uncertainty: 0,
                probes: 0,
                teller: 0,
            };
        }
    }

    /// Stores a place inferred from another agent's gesture, with a confidence
    /// that reflects trust in the teller. Returns whether the map changed.
    /// First-hand memories are never displaced by hearsay.
    pub(crate) fn remember_told(
        &mut self,
        kind: LandmarkKind,
        estimate: WorldPosition,
        uncertainty: u8,
        now: u32,
        teller_slot: Option<u8>,
        confidence: u8,
    ) -> bool {
        let Some((x, y)) = compact(estimate) else {
            return false;
        };
        let radius = u64::from(uncertainty) * 4;
        let range = slot_range(kind);
        for slot in range.clone() {
            let landmark = &mut self.landmarks[slot];
            if landmark.is_empty()
                || chebyshev(landmark.position(), estimate)
                    > landmark.radius() + radius + MERGE_RADIUS
            {
                continue;
            }
            if landmark.is_first_hand() {
                return false;
            }
            landmark.confidence = landmark.confidence.saturating_add(32);
            landmark.seen = now;
            return true;
        }
        let slot = self.replacement_slot(range.clone(), false).or_else(|| {
            // A fresh hint can displace a first-hand memory the agent now trusts
            // less, such as an old food sighting that has probably been eaten.
            range
                .map(|slot| (believed_confidence(self.landmarks[slot], kind, now), slot))
                .filter(|&(belief, _)| belief < u64::from(confidence))
                .min()
                .map(|(_, slot)| slot)
        });
        let Some(slot) = slot else {
            return false;
        };
        self.landmarks[slot] = Landmark {
            x,
            y,
            seen: now,
            confidence: confidence.max(FORGET_CONFIDENCE),
            uncertainty: uncertainty.max(1),
            probes: 0,
            teller: teller_slot.map_or(0, |slot| slot + 1),
        };
        true
    }

    /// Detaches hints from an acquaintance slot that now holds someone else.
    pub(crate) fn forget_teller(&mut self, slot: u8) {
        for landmark in &mut self.landmarks {
            if landmark.teller == slot + 1 {
                landmark.teller = 0;
            }
        }
    }

    /// An empty slot, else the weakest hint, else (only for first-hand
    /// observations) the least recently confirmed memory.
    fn replacement_slot(&self, range: std::ops::Range<usize>, first_hand: bool) -> Option<usize> {
        if let Some(slot) = range.clone().find(|&slot| self.landmarks[slot].is_empty()) {
            return Some(slot);
        }
        if let Some(slot) = range
            .clone()
            .filter(|&slot| !self.landmarks[slot].is_first_hand())
            .min_by_key(|&slot| (self.landmarks[slot].confidence, self.landmarks[slot].seen))
        {
            return Some(slot);
        }
        first_hand.then(|| range.min_by_key(|&slot| self.landmarks[slot].seen))?
    }

    /// The remembered place of `kind` most worth travelling to from `origin`, as a
    /// concrete destination (hints are searched around their estimate). Places
    /// compete by expected cost: distance plus search effort, divided by how much
    /// the agent still believes in them. Food sightings go stale (it gets eaten).
    pub(crate) fn recall(
        &self,
        kind: LandmarkKind,
        agent: u32,
        origin: WorldPosition,
        now: u32,
    ) -> Option<(WorldPosition, LandmarkSource)> {
        slot_range(kind)
            .filter(|&slot| !self.landmarks[slot].is_empty())
            .map(|slot| {
                let landmark = self.landmarks[slot];
                let (destination, source) = if landmark.is_first_hand() {
                    (landmark.position(), LandmarkSource::Seen)
                } else {
                    (probe_point(landmark, agent, slot), LandmarkSource::Told)
                };
                let belief = believed_confidence(landmark, kind, now);
                let score = (manhattan(origin, destination) + landmark.radius()) * 256 / belief;
                (score, slot, destination, source)
            })
            .min_by_key(|&(score, slot, _, _)| (score, slot))
            .map(|(_, _, destination, source)| (destination, source))
    }

    /// The most believable hint the agent hasn't checked yet, as a destination:
    /// what a curious agent goes to see for itself.
    pub(crate) fn hint_to_check(&self, agent: u32, origin: WorldPosition) -> Option<WorldPosition> {
        (0..LANDMARK_SLOTS)
            .filter(|&slot| {
                let landmark = self.landmarks[slot];
                !landmark.is_empty() && !landmark.is_first_hand() && landmark.probes == 0
            })
            .map(|slot| {
                let landmark = self.landmarks[slot];
                let destination = probe_point(landmark, agent, slot);
                let cost = (manhattan(origin, destination) + landmark.radius()) * 256
                    / u64::from(landmark.confidence.max(16));
                (cost, slot, destination)
            })
            .min()
            .map(|(_, _, destination)| destination)
    }

    /// Whether the agent remembers any place of `kind` near `position`.
    pub(crate) fn remembers_near(
        &self,
        kind: LandmarkKind,
        position: WorldPosition,
        radius: u64,
    ) -> bool {
        slot_range(kind).any(|slot| {
            let landmark = self.landmarks[slot];
            !landmark.is_empty()
                && chebyshev(landmark.position(), position)
                    <= radius + landmark.radius() + MERGE_RADIUS
        })
    }

    /// Distance to the nearest first-hand memory of `kind`.
    pub(crate) fn nearest_seen_distance(
        &self,
        kind: LandmarkKind,
        origin: WorldPosition,
    ) -> Option<u64> {
        slot_range(kind)
            .map(|slot| self.landmarks[slot])
            .filter(|landmark| !landmark.is_empty() && landmark.is_first_hand())
            .map(|landmark| manhattan(origin, landmark.position()))
            .min()
    }

    pub(crate) fn seen_count(&self, kind: LandmarkKind) -> usize {
        slot_range(kind)
            .filter(|&slot| {
                let landmark = self.landmarks[slot];
                !landmark.is_empty() && landmark.is_first_hand()
            })
            .count()
    }

    /// The kind of first-hand memory at exactly `position`, if any.
    pub(crate) fn seen_kind_at(&self, position: WorldPosition) -> Option<LandmarkKind> {
        (0..LANDMARK_SLOTS)
            .find(|&slot| {
                let landmark = self.landmarks[slot];
                !landmark.is_empty() && landmark.is_first_hand() && landmark.position() == position
            })
            .map(kind_of_slot)
    }

    /// Next first-hand place worth pointing out to someone nearby: outside the
    /// current view (they could see it themselves otherwise), rotating through
    /// memories so repeated encounters share different places. Returns the place
    /// and its rotation rank for `mark_shared`.
    pub(crate) fn shareable(&self, view: WorldRect) -> Option<(WorldPosition, u8)> {
        let order = [
            LandmarkKind::Water,
            LandmarkKind::Food,
            LandmarkKind::Shelter,
            LandmarkKind::Wood,
            LandmarkKind::Stone,
        ];
        let mut candidates = order
            .iter()
            .flat_map(|&kind| slot_range(kind))
            .filter(|&slot| {
                let landmark = self.landmarks[slot];
                !landmark.is_empty()
                    && landmark.is_first_hand()
                    && !contains(view, landmark.position())
            })
            .map(|slot| self.landmarks[slot].position())
            .chain(self.recent_explored_markers(view))
            .enumerate()
            .map(|(rank, place)| (place, rank as u8));
        let first = candidates.next()?;
        if first.1 >= self.share_cursor {
            return Some(first);
        }
        Some(
            candidates
                .find(|&(_, rank)| rank >= self.share_cursor)
                .unwrap_or(first),
        )
    }

    /// Centers of the three most recently explored tiles outside `view`: what an
    /// agent can sweep a hand over to say "I've been over there".
    fn recent_explored_markers(&self, view: WorldRect) -> impl Iterator<Item = WorldPosition> + '_ {
        let len = usize::from(self.visited_len);
        let newest = usize::from(self.visited_cursor) + VISITED_TILE_SLOTS;
        (1..=len)
            .map(move |back| self.visited[(newest - back) % VISITED_TILE_SLOTS])
            .map(tile_center)
            .filter(move |center| !contains(view, *center))
            .take(3)
    }

    /// Whether `position` is the center of an explored tile (an "explored" gesture target).
    pub(crate) fn is_explored_marker(&self, position: WorldPosition) -> bool {
        let tile = tile_of(position);
        tile_center(tile) == position && self.visited(tile)
    }

    pub(crate) fn share_ready(&self, now: u32, cooldown: u32) -> bool {
        self.last_share == u32::MAX || now.saturating_sub(self.last_share) >= cooldown
    }

    pub(crate) fn mark_shared(&mut self, now: u32, rank: u8) {
        self.last_share = now;
        self.share_cursor = rank.wrapping_add(1);
    }

    /// Marks the tile containing `position` explored. Returns whether it was new.
    pub(crate) fn record_visit(&mut self, position: WorldPosition) -> bool {
        let tile = tile_of(position);
        let len = usize::from(self.visited_len);
        if self.visited[..len].contains(&tile) {
            return false;
        }
        let cursor = usize::from(self.visited_cursor);
        self.visited[cursor] = tile;
        self.visited_cursor = ((cursor + 1) % VISITED_TILE_SLOTS) as u8;
        self.visited_len = (len + 1).min(VISITED_TILE_SLOTS) as u8;
        true
    }

    fn visited(&self, tile: (i16, i16)) -> bool {
        self.visited[..usize::from(self.visited_len)].contains(&tile)
    }

    /// The direction closest to `preferred` whose look-ahead tile is unexplored.
    pub(crate) fn novel_heading(
        &self,
        origin: WorldPosition,
        preferred: ExplorationHeading,
    ) -> Option<ExplorationHeading> {
        [0_i8, 1, -1, 2, -2, 3, -3, 4]
            .into_iter()
            .map(|turn| preferred.rotated(turn))
            .find(|heading| {
                let (dx, dy) = heading.delta();
                !self.visited(tile_of(WorldPosition {
                    x: origin.x + dx * NOVELTY_LOOKAHEAD,
                    y: origin.y + dy * NOVELTY_LOOKAHEAD,
                }))
            })
    }

    /// Next corner of an outward square spiral around where the search began.
    /// Corners already in view, or that resisted too many decisions, are skipped.
    pub(crate) fn search_target(
        &mut self,
        origin: WorldPosition,
        view: WorldRect,
    ) -> WorldPosition {
        if self.search_step == NO_SEARCH {
            if let Some(anchor) = compact(origin) {
                self.search_anchor = anchor;
            }
            self.search_step = 0;
            self.search_leg_decisions = 0;
        }
        loop {
            let corner = spiral_corner(self.search_anchor, self.search_step);
            let exhausted = self.search_leg_decisions >= SEARCH_LEG_DECISIONS;
            if (contains(view, corner) || exhausted) && self.search_step < NO_SEARCH - 1 {
                self.search_step += 1;
                self.search_leg_decisions = 0;
                continue;
            }
            self.search_leg_decisions = self.search_leg_decisions.saturating_add(1);
            return corner;
        }
    }

    /// Ends any spiral search; the next one starts from wherever the agent is then.
    pub(crate) fn end_search(&mut self) {
        self.search_step = NO_SEARCH;
    }

    pub(crate) fn views(&self) -> impl Iterator<Item = LandmarkView> + '_ {
        (0..LANDMARK_SLOTS)
            .filter(|&slot| !self.landmarks[slot].is_empty())
            .map(|slot| {
                let landmark = self.landmarks[slot];
                LandmarkView {
                    kind: kind_of_slot(slot),
                    position: landmark.position(),
                    source: if landmark.is_first_hand() {
                        LandmarkSource::Seen
                    } else {
                        LandmarkSource::Told
                    },
                    confidence: landmark.confidence,
                    search_radius: landmark.radius() as u16,
                    seen_second: landmark.seen,
                }
            })
    }

    pub(crate) fn explored_tile_count(&self) -> usize {
        usize::from(self.visited_len)
    }
}

/// Corner `step` of a square spiral: legs of 1, 1, 2, 2, 3, 3, ... spacings,
/// turning east, south, west, north.
fn spiral_corner(anchor: (i16, i16), step: u16) -> WorldPosition {
    let (mut x, mut y) = (i64::from(anchor.0), i64::from(anchor.1));
    for leg in 0..i64::from(step) {
        let length = (leg / 2 + 1) * SEARCH_SPACING;
        match leg % 4 {
            0 => x += length,
            1 => y += length,
            2 => x -= length,
            _ => y -= length,
        }
    }
    WorldPosition { x, y }
}

/// How much the agent still believes a place holds what it remembers. Food
/// sightings lose belief as they age; other places stay as remembered.
fn believed_confidence(landmark: Landmark, kind: LandmarkKind, now: u32) -> u64 {
    let age = u64::from(now.saturating_sub(landmark.seen));
    let decay = if kind == LandmarkKind::Food && landmark.is_first_hand() {
        (age / FOOD_STALENESS_SECONDS).min(180)
    } else {
        0
    };
    u64::from(landmark.confidence).saturating_sub(decay).max(16)
}

fn tile_center(tile: (i16, i16)) -> WorldPosition {
    WorldPosition {
        x: i64::from(tile.0) * VISIT_TILE_SIZE + VISIT_TILE_SIZE / 2,
        y: i64::from(tile.1) * VISIT_TILE_SIZE + VISIT_TILE_SIZE / 2,
    }
}

fn compact(position: WorldPosition) -> Option<(i16, i16)> {
    Some((
        i16::try_from(position.x).ok()?,
        i16::try_from(position.y).ok()?,
    ))
}

/// Where to look next for a hint: its estimate first, then deterministic points
/// spread inside its search radius.
fn probe_point(landmark: Landmark, agent: u32, slot: usize) -> WorldPosition {
    let center = landmark.position();
    if landmark.probes == 0 {
        return center;
    }
    let radius = landmark.radius().max(1) as i64;
    let mut key = u64::from(agent) << 32 | (slot as u64) << 8 | u64::from(landmark.probes);
    key = (key ^ (key >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    key = (key ^ (key >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    key ^= key >> 31;
    let span = (radius * 2 + 1) as u64;
    WorldPosition {
        x: center.x + (key % span) as i64 - radius,
        y: center.y + ((key >> 32) % span) as i64 - radius,
    }
}
