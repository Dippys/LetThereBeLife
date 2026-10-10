//! Conversation memory for learning from consequences (spec 05, "Communication
//! success is inferred later", "Clarification and repair").
//!
//! Each hint carries the word it came with (see `map::HintSource`), so finding
//! out what was really there can correct that word. If a hint turned out to be a
//! misunderstanding, the listener remembers to set the record straight the next
//! time it meets the speaker.

use crate::{AgentId, WorldPosition};

use super::{Concept, VocalForm};

/// Evidence a consequence adds to (or takes from) a word.
pub const CONSEQUENCE_WEIGHT: u16 = 3;
/// Evidence a confirmation or repair adds to (or takes from) a word.
pub const REPAIR_WEIGHT: u16 = 2;
/// Corrections older than this (simulated seconds) are dropped.
const CORRECTION_PATIENCE_SECONDS: u32 = 3_600;

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
}

impl Dialogue {
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
            actual: Concept::Food,
            place: at(0, 0),
            since: 100,
        });
        assert!(dialogue.correction(200).is_some());
        assert!(dialogue.correction(100 + 3_601).is_none());
    }
}
