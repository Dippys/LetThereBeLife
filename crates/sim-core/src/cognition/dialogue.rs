//! Conversation memory for learning from consequences (spec 05, "Communication
//! success is inferred later", "Clarification and repair").
//!
//! Each hint carries the word it came with (see `map::HintSource`), so finding
//! out what was really there can correct that word. If a hint turned out to be a
//! misunderstanding, the listener remembers to set the record straight the next
//! time it meets the speaker. It also tracks the agent's own requests for food.

use crate::{AgentId, WorldPosition, agent::CompactPosition};

use super::{Concept, VocalForm};

/// Evidence a consequence adds to (or takes from) a word.
pub const CONSEQUENCE_WEIGHT: u16 = 3;
/// Evidence a confirmation or repair adds to (or takes from) a word.
pub const REPAIR_WEIGHT: u16 = 2;
/// Corrections older than this (simulated seconds) are dropped.
const CORRECTION_PATIENCE_SECONDS: u32 = 6 * 3_600;
/// A tip about an animal is worth acting on for this many seconds.
pub const LEAD_SECONDS: u32 = 90;
/// Seconds between warnings, and between calls to hunt.
const WARNING_COOLDOWN_SECONDS: u32 = 30;
const RECRUIT_COOLDOWN_SECONDS: u32 = 90;

/// Something someone pointed out about an animal: what the listener took it to
/// be, roughly where, until when it's worth acting on, and where the tip came
/// from (so finding something else there can teach about the word).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Lead {
    pub(crate) signal: u64,
    pub(crate) speaker: AgentId,
    pub(crate) until: u32,
    pub(crate) place: CompactPosition,
    pub(crate) species: crate::Species,
    pub(crate) form: Option<VocalForm>,
    /// The speaker snarled (it meant something dangerous).
    pub(crate) warned: bool,
}

impl Lead {
    pub(crate) fn place(self) -> WorldPosition {
        self.place.world()
    }
}

/// Seconds before asking again after a request was answered.
const REQUEST_COOLDOWN_SECONDS: u32 = 120;
/// Seconds before asking again after a refusal.
const REFUSED_COOLDOWN_SECONDS: u32 = 600;

/// What a listener plans to tell a speaker after a misunderstanding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PendingCorrection {
    pub(crate) speaker: AgentId,
    pub(crate) form: VocalForm,
    /// What the listener took the word to mean.
    pub(crate) misread: Concept,
    /// What was actually there.
    pub(crate) actual: Concept,
    pub(crate) place: WorldPosition,
    pub(crate) since: u32,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Dialogue {
    correction: Option<PendingCorrection>,
    /// Who the agent is about to ask for food, and where they stood.
    request: Option<(AgentId, CompactPosition)>,
    /// Simulated second from which the agent may ask again.
    next_request: u32,
    /// A warned-about animal to stay away from, and one to go and hunt.
    pub(crate) alarm: Option<Lead>,
    pub(crate) quarry: Option<Lead>,
    /// An animal the agent is about to point out, and where it was.
    animal: Option<(crate::Species, CompactPosition)>,
    /// Simulated seconds from which it may warn, or call others to hunt, again.
    next_warning: u32,
    next_recruit: u32,
}

impl Dialogue {
    pub(crate) const fn may_warn(&self, now: u32) -> bool {
        now >= self.next_warning
    }

    pub(crate) const fn may_recruit(&self, now: u32) -> bool {
        now >= self.next_recruit
    }

    /// Plans to point out an animal (a warning or a call to hunt) and starts
    /// the matching cooldown.
    pub(crate) fn plan_animal(
        &mut self,
        species: crate::Species,
        place: WorldPosition,
        warning: bool,
        now: u32,
    ) {
        self.animal = CompactPosition::checked(place).map(|place| (species, place));
        if warning {
            self.next_warning = now.saturating_add(WARNING_COOLDOWN_SECONDS);
        } else {
            self.next_recruit = now.saturating_add(RECRUIT_COOLDOWN_SECONDS);
        }
    }

    /// The animal planned to be pointed out at `place`, consumed.
    pub(crate) fn take_animal(&mut self, place: WorldPosition) -> Option<crate::Species> {
        self.animal.take().and_then(|(species, at)| {
            (Some(at) == CompactPosition::checked(place)).then_some(species)
        })
    }

    /// Leads still worth acting on (expired ones are dropped).
    pub(crate) fn current_leads(&mut self, now: u32) -> (Option<Lead>, Option<Lead>) {
        if self.alarm.is_some_and(|lead| lead.until < now) {
            self.alarm = None;
        }
        if self.quarry.is_some_and(|lead| lead.until < now) {
            self.quarry = None;
        }
        (self.alarm, self.quarry)
    }

    pub(crate) const fn may_request(&self, now: u32) -> bool {
        now >= self.next_request
    }

    pub(crate) fn plan_request(&mut self, addressee: AgentId, place: WorldPosition) {
        self.request = CompactPosition::checked(place).map(|place| (addressee, place));
    }

    /// The request planned toward `place`, if any. Any planned request is
    /// consumed (one aimed elsewhere was abandoned).
    pub(crate) fn take_request(&mut self, place: WorldPosition) -> Option<AgentId> {
        self.request.take().and_then(|(addressee, at)| {
            (Some(at) == CompactPosition::checked(place)).then_some(addressee)
        })
    }

    pub(crate) fn request_answered(&mut self, now: u32, refused: bool) {
        self.next_request = now.saturating_add(if refused {
            REFUSED_COOLDOWN_SECONDS
        } else {
            REQUEST_COOLDOWN_SECONDS
        });
    }

    pub(crate) fn plan_correction(&mut self, correction: PendingCorrection) {
        self.correction = Some(correction);
    }

    /// The correction still worth making, if any.
    pub(crate) fn correction(&self, now: u32) -> Option<PendingCorrection> {
        self.correction.filter(|correction| {
            now.saturating_sub(correction.since) <= CORRECTION_PATIENCE_SECONDS
        })
    }

    pub(crate) fn clear_correction(&mut self) {
        self.correction = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: i64, y: i64) -> WorldPosition {
        WorldPosition { x, y }
    }

    #[test]
    fn corrections_expire() {
        let mut dialogue = Dialogue::default();
        dialogue.plan_correction(PendingCorrection {
            speaker: AgentId::new(1),
            form: VocalForm(2),
            misread: Concept::Water,
            actual: Concept::Berries,
            place: at(0, 0),
            since: 100,
        });
        assert!(dialogue.correction(200).is_some());
        assert!(dialogue.correction(100 + 6 * 3_600).is_some());
        assert!(dialogue.correction(100 + 6 * 3_600 + 1).is_none());
    }
}
