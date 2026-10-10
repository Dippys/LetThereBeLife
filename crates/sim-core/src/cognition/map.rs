//! The per-agent mental map: a fixed number of remembered places per kind,
//! a ring of recently explored tiles, and sharing bookkeeping. Everything here
//! is the agent's private belief, never authoritative world state.

use crate::{
    PhysicalPerception, WorldPosition, WorldRect,
    policy::ExplorationHeading,
    structures::{StructureKind, StructureState},
};

use super::{LandmarkKind, LandmarkSource, LandmarkView};

/// Remembered-place slots per agent, partitioned by kind (see `slot_range`).
pub const LANDMARK_SLOTS: usize = WATER_SLOTS + KIND_SLOTS * (LandmarkKind::COUNT - 1);
/// Recently explored tiles remembered for novelty-seeking exploration.
pub const VISITED_TILE_SLOTS: usize = 24;
/// Edge length of an exploration tile in cells.
pub const VISIT_TILE_SIZE: i64 = 32;
/// Same-kind places closer than this (Chebyshev cells) merge into one memory.
pub const MERGE_RADIUS: u64 = 24;
/// A worded hint names one spot: what stands this close to it (Chebyshev
/// cells) is what was pointed at.
pub const SPOT_RADIUS: u64 = 2;
/// From this close (Chebyshev cells) an agent can see what stands at a spot.
const SPOT_VIEW: u64 = 6;
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
/// marks an empty slot. Exactly 16 bytes.
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
    /// Low 4 bits: searches made so far. High 4 bits: pointing bearing + 1
    /// (an 8-way heading; 0 = unknown), so searches follow the pointed line.
    probes: u8,
    /// For hints: acquaintance slot + 1 of whoever pointed it out (0 = unknown).
    teller: u8,
    /// For hints: the word it came with (`NONE` = no word).
    form: u8,
    /// For hints: the runner-up meaning the listener also weighed (`NONE` = none).
    alternative: u8,
    /// For hints: low 16 bits of the gesture id it came from, for logs.
    signal: u16,
}

const NONE: u8 = u8::MAX;

/// Where a hint came from. Kept with the hint so that checking it later can
/// teach the listener about the word and the teller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HintSource {
    pub(crate) teller: Option<u8>,
    pub(crate) form: Option<crate::VocalForm>,
    pub(crate) alternative: Option<crate::Concept>,
    pub(crate) signal: u64,
    /// The direction the teller pointed, if known.
    pub(crate) bearing: Option<ExplorationHeading>,
}

impl HintSource {
    #[cfg(test)]
    pub(crate) const fn anonymous() -> Self {
        Self {
            teller: None,
            form: None,
            alternative: None,
            signal: 0,
            bearing: None,
        }
    }

    #[cfg(test)]
    pub(crate) const fn from_teller(slot: u8) -> Self {
        Self {
            teller: Some(slot),
            ..Self::anonymous()
        }
    }
}

/// A hint the agent just checked by looking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HintCheck {
    pub(crate) kind: LandmarkKind,
    pub(crate) confirmed: bool,
    pub(crate) teller: Option<u8>,
    pub(crate) form: Option<crate::VocalForm>,
    pub(crate) alternative: Option<crate::Concept>,
    /// Low 16 bits of the gesture id.
    pub(crate) signal: u16,
    /// Where the hint said to look.
    pub(crate) place: WorldPosition,
    /// Judged by looking at the spot itself (not given up after searching).
    pub(crate) up_close: bool,
}

impl Landmark {
    const fn probe_count(self) -> u8 {
        self.probes & 0x0F
    }

    fn bearing(self) -> Option<ExplorationHeading> {
        let packed = self.probes >> 4;
        (packed != 0).then(|| ExplorationHeading::North.rotated((packed - 1) as i8))
    }

    fn add_probe(&mut self) {
        self.probes = (self.probes & 0xF0) | (self.probe_count() + 1).min(0x0F);
    }

    fn check(self, kind: LandmarkKind, confirmed: bool) -> HintCheck {
        HintCheck {
            kind,
            confirmed,
            teller: (self.teller != 0).then(|| self.teller - 1),
            form: (self.form != NONE).then_some(crate::VocalForm(self.form)),
            alternative: (self.alternative != NONE).then(|| {
                crate::Concept::ALL[usize::from(self.alternative) % crate::Concept::COUNT]
            }),
            signal: self.signal,
            place: self.position(),
            up_close: false,
        }
    }
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
    /// Where it stood at its last decision, so it doesn't step straight back
    /// there (pacing between two spots around an obstacle).
    trail: (i16, i16),
    has_trail: bool,
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
            trail: (0, 0),
            has_trail: false,
        }
    }
}

/// Slots for remembered water; every other kind of place gets `KIND_SLOTS`.
const WATER_SLOTS: usize = 4;
const KIND_SLOTS: usize = 2;

const fn slot_range(kind: LandmarkKind) -> std::ops::Range<usize> {
    match kind {
        LandmarkKind::Water => 0..WATER_SLOTS,
        _ => {
            let start = WATER_SLOTS + (kind.index() - 1) * KIND_SLOTS;
            start..start + KIND_SLOTS
        }
    }
}

const fn kind_of_slot(slot: usize) -> LandmarkKind {
    if slot < WATER_SLOTS {
        LandmarkKind::Water
    } else {
        LandmarkKind::ALL[1 + (slot - WATER_SLOTS) / KIND_SLOTS]
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

/// Which resource kinds have picked-clean instances in view, in kind order.
pub(crate) fn spent_kinds(perception: &PhysicalPerception) -> [bool; LandmarkKind::COUNT] {
    let mut spent = [false; LandmarkKind::COUNT];
    for resource in &perception.spent_resources {
        if let Some(kind) = LandmarkKind::of_material(resource.resource.kind) {
            spent[kind.index()] = true;
        }
    }
    spent
}

/// Which landmark kinds are in view right now, in kind order.
pub(crate) fn visible_kinds(
    origin: WorldPosition,
    perception: &PhysicalPerception,
) -> [bool; LandmarkKind::COUNT] {
    perceived_nearest(origin, perception).map(|nearest| nearest.is_some())
}

/// Every perceived instance of `kind`.
fn perceived_instances<'a>(
    kind: LandmarkKind,
    perception: &'a PhysicalPerception,
) -> Box<dyn Iterator<Item = WorldPosition> + 'a> {
    match (kind, kind.material()) {
        (_, Some(material)) => Box::new(
            perception
                .resources
                .iter()
                .filter(move |resource| resource.resource.kind == material)
                .map(|resource| resource.position),
        ),
        (LandmarkKind::Water, None) => Box::new(
            perception
                .drinkable_water
                .iter()
                .map(|water| water.position),
        ),
        (_, None) => {
            let wanted = if kind == LandmarkKind::HEARTH {
                StructureKind::Hearth
            } else {
                StructureKind::Shelter
            };
            Box::new(
                perception
                    .structures
                    .iter()
                    .filter(move |structure| {
                        structure.state == StructureState::Complete && structure.kind == wanted
                    })
                    .map(|structure| structure.position),
            )
        }
    }
}

/// Nearest perceived instance of each landmark kind, in kind order.
fn perceived_nearest(
    origin: WorldPosition,
    perception: &PhysicalPerception,
) -> [Option<WorldPosition>; LandmarkKind::COUNT] {
    LandmarkKind::ALL.map(|kind| {
        perceived_instances(kind, perception)
            .min_by_key(|position| (manhattan(origin, *position), position.y, position.x))
    })
}

/// How close (Chebyshev cells) the nearest instance of `kind` in view stands to
/// `spot`, if one stands within `SPOT_RADIUS`.
fn distance_to_spot(
    kind: LandmarkKind,
    spot: WorldPosition,
    perception: &PhysicalPerception,
) -> Option<u64> {
    perceived_instances(kind, perception)
        .map(|position| chebyshev(position, spot))
        .filter(|&distance| distance <= SPOT_RADIUS)
        .min()
}

impl MentalMap {
    /// Updates beliefs from one perception: forgets places that turned out empty,
    /// remembers the nearest visible place of each kind, and marks the tile explored.
    /// `on_hint` reports hints that were confirmed by
    /// seeing the place or abandoned after failed searches.
    pub(crate) fn observe(
        &mut self,
        agent: u32,
        origin: WorldPosition,
        perception: &PhysicalPerception,
        now: u32,
        on_hint: &mut impl FnMut(HintCheck),
    ) {
        let _ = self.record_visit(origin);
        self.check_spots(origin, perception, on_hint);
        let nearest = perceived_nearest(origin, perception);
        for slot in 0..LANDMARK_SLOTS {
            let landmark = self.landmarks[slot];
            if landmark.is_empty() || nearest[kind_of_slot(slot).index()].is_some() {
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
                slot_ref.add_probe();
                slot_ref.confidence = slot_ref.confidence.saturating_sub(FAILED_PROBE_PENALTY);
                if slot_ref.confidence < FORGET_CONFIDENCE || slot_ref.probe_count() >= MAX_PROBES {
                    on_hint(slot_ref.check(kind_of_slot(slot), false));
                    *slot_ref = Landmark::default();
                }
            }
        }
        for kind in LandmarkKind::ALL {
            if let Some(position) = nearest[kind.index()] {
                self.remember_seen(kind, position, now, on_hint);
            }
        }
    }

    /// Worded hints whose spot is close enough to see: the expected thing standing
    /// closest to the spot confirms the hint; the alternative the listener
    /// weighed, standing closer, shows the word was misread.
    fn check_spots(
        &mut self,
        origin: WorldPosition,
        perception: &PhysicalPerception,
        on_hint: &mut impl FnMut(HintCheck),
    ) {
        for slot in 0..LANDMARK_SLOTS {
            let landmark = self.landmarks[slot];
            if landmark.is_empty() || landmark.is_first_hand() || landmark.form == NONE {
                continue;
            }
            let spot = landmark.position();
            if chebyshev(origin, spot) > SPOT_VIEW || !contains(perception.area, spot) {
                continue;
            }
            let kind = kind_of_slot(slot);
            let unconfirmed = landmark.check(kind, false);
            let alternative = unconfirmed.alternative.and_then(|concept| {
                match crate::cognition::concept_topic(concept) {
                    Some(crate::GestureTopic::Place(other)) if other != kind => Some(other),
                    _ => None,
                }
            });
            // What was pointed at is whichever of the two stands closest to the spot.
            let expected = distance_to_spot(kind, spot, perception);
            let other = alternative.and_then(|other| distance_to_spot(other, spot, perception));
            let check = match (expected, other) {
                (Some(near), Some(far)) if near <= far => landmark.check(kind, true),
                (Some(_), None) => landmark.check(kind, true),
                // The alternative at the spot and nothing of the expected kind
                // anywhere near it: the word must have meant that.
                (None, Some(_))
                    if !perceived_instances(kind, perception)
                        .any(|position| chebyshev(position, spot) <= SPOT_VIEW) =>
                {
                    unconfirmed
                }
                // Both around: pointing is too rough to tell which was meant.
                (_, Some(_)) | (None, None) => {
                    // Nothing telling at the spot (the pointing was off, or it's
                    // gone): from now on it's an ordinary hint, searched as usual.
                    self.landmarks[slot].form = NONE;
                    continue;
                }
            };
            on_hint(HintCheck {
                up_close: true,
                ..check
            });
            self.landmarks[slot] = Landmark::default();
        }
    }

    fn remember_seen(
        &mut self,
        kind: LandmarkKind,
        position: WorldPosition,
        now: u32,
        on_hint: &mut impl FnMut(HintCheck),
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
            // A hint that this sighting explains is now confirmed and replaced
            // below. A worded hint named one spot and is judged only up close
            // (`check_spots`).
            if !landmark.is_first_hand()
                && landmark.form == NONE
                && chebyshev(landmark.position(), position) <= landmark.radius() + MERGE_RADIUS
            {
                on_hint(landmark.check(kind, true));
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
                form: NONE,
                alternative: NONE,
                signal: 0,
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
        source: HintSource,
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
                // A worded hint names one spot: a known place elsewhere nearby
                // doesn't make it old news.
                if source.form.is_some() && chebyshev(landmark.position(), estimate) > SPOT_RADIUS {
                    continue;
                }
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
            probes: source.bearing.map_or(0, |bearing| (bearing as u8 + 1) << 4),
            teller: source.teller.map_or(0, |slot| slot + 1),
            form: source.form.map_or(NONE, |form| form.0),
            alternative: source
                .alternative
                .map_or(NONE, |concept| concept.index() as u8),
            signal: source.signal as u16,
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
    #[cfg(test)]
    pub(crate) fn recall(
        &self,
        kind: LandmarkKind,
        agent: u32,
        origin: WorldPosition,
        now: u32,
    ) -> Option<(WorldPosition, LandmarkSource)> {
        self.recall_scored(kind, agent, origin, now, false)
            .map(|(_, destination, source)| (destination, source))
    }

    /// Like `recall`, with the expected cost, so places of different kinds can
    /// compete. With `seen_only`, hints are ignored.
    pub(crate) fn recall_scored(
        &self,
        kind: LandmarkKind,
        agent: u32,
        origin: WorldPosition,
        now: u32,
        seen_only: bool,
    ) -> Option<(u64, WorldPosition, LandmarkSource)> {
        slot_range(kind)
            .filter(|&slot| {
                let landmark = self.landmarks[slot];
                !landmark.is_empty() && (!seen_only || landmark.is_first_hand())
            })
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
            .map(|(score, _, destination, source)| (score, destination, source))
    }

    /// The most believable hint the agent hasn't checked yet, as a destination:
    /// what a curious agent goes to see for itself.
    pub(crate) fn hint_to_check(&self, agent: u32, origin: WorldPosition) -> Option<WorldPosition> {
        (0..LANDMARK_SLOTS)
            .filter(|&slot| {
                let landmark = self.landmarks[slot];
                !landmark.is_empty() && !landmark.is_first_hand() && landmark.probe_count() == 0
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

    /// Drops hints of `kind` around `position` (after learning they were misread).
    pub(crate) fn forget_hint(&mut self, kind: LandmarkKind, position: WorldPosition) {
        for slot in slot_range(kind) {
            let landmark = self.landmarks[slot];
            if !landmark.is_empty()
                && !landmark.is_first_hand()
                && chebyshev(landmark.position(), position) <= landmark.radius() + MERGE_RADIUS
            {
                self.landmarks[slot] = Landmark::default();
            }
        }
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
            LandmarkKind::BERRIES,
            LandmarkKind::SHELTER,
            LandmarkKind::HEARTH,
            LandmarkKind::WOOD,
            LandmarkKind::BITTERBERRIES,
            LandmarkKind::STONE,
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

    /// Where the agent stood at its previous decision, if it has moved since.
    pub(crate) fn came_from(&self, origin: WorldPosition) -> Option<WorldPosition> {
        let previous = WorldPosition {
            x: i64::from(self.trail.0),
            y: i64::from(self.trail.1),
        };
        (self.has_trail && previous != origin).then_some(previous)
    }

    /// Remembers where it stands now for the next decision.
    pub(crate) fn mark_decision(&mut self, origin: WorldPosition) {
        if let Some(position) = compact(origin) {
            self.trail = position;
            self.has_trail = true;
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

/// How much the agent still believes a place holds what it remembers. Bush
/// sightings lose belief as they age (others pick them); other places stay.
fn believed_confidence(landmark: Landmark, kind: LandmarkKind, now: u32) -> u64 {
    let age = u64::from(now.saturating_sub(landmark.seen));
    let decay = if matches!(kind, LandmarkKind::BERRIES | LandmarkKind::BITTERBERRIES)
        && landmark.is_first_hand()
    {
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

/// Where to look next for a hint: its estimate first, then points spread along
/// the pointed line (distance was the vague part) with a little sideways
/// jitter, or anywhere inside the search radius if the bearing is unknown.
fn probe_point(landmark: Landmark, agent: u32, slot: usize) -> WorldPosition {
    let center = landmark.position();
    let count = landmark.probe_count();
    if count == 0 {
        return center;
    }
    let radius = landmark.radius().max(1) as i64;
    let mut key = u64::from(agent) << 32 | (slot as u64) << 8 | u64::from(count);
    key = (key ^ (key >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    key = (key ^ (key >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    key ^= key >> 31;
    if let Some(bearing) = landmark.bearing() {
        const ALONG: [i64; 8] = [0, 1, -1, 2, -2, 3, -3, 0];
        let (dx, dy) = bearing.delta();
        let step = (radius / 3).max(4);
        let along = ALONG[usize::from(count).min(7)] * step;
        let sideways = (key % 9) as i64 - 4;
        return WorldPosition {
            x: center.x + dx * along - dy * sideways,
            y: center.y + dy * along + dx * sideways,
        };
    }
    let span = (radius * 2 + 1) as u64;
    WorldPosition {
        x: center.x + (key % span) as i64 - radius,
        y: center.y + ((key >> 32) % span) as i64 - radius,
    }
}
