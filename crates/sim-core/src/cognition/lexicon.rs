//! Concepts, vocal forms, and personal lexicons (spec 06).
//!
//! A concept is an internal category; a vocal form is an observable sound.
//! Each agent owns a small lexicon of `form → concept` hypotheses with evidence
//! for and against, kept separately for hearing (recognition) and speaking
//! (production). Founders inherit a noisy proto-language; there is no global
//! dictionary — the "community language" is only the overlap between people.

use crate::AgentId;

/// Internal categories agents can think and talk about. Ids are an engine
/// convenience; agents may come to link them to different forms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Concept {
    Water = 0,
    Food = 1,
    Wood = 2,
    Stone = 3,
    Home = 4,
    /// "I've been over there."
    Been = 5,
    Come = 6,
    Give = 7,
    Me = 8,
    You = 9,
    Yes = 10,
    No = 11,
}

impl Concept {
    pub const COUNT: usize = 12;
    pub const ALL: [Self; Self::COUNT] = [
        Self::Water,
        Self::Food,
        Self::Wood,
        Self::Stone,
        Self::Home,
        Self::Been,
        Self::Come,
        Self::Give,
        Self::Me,
        Self::You,
        Self::Yes,
        Self::No,
    ];

    const fn from_index(index: u8) -> Self {
        Self::ALL[index as usize % Self::COUNT]
    }
}

/// An observable sound. Only the id matters to agents; `name` makes logs readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VocalForm(pub u8);

/// How many distinct forms exist (more than concepts, so variants can arise).
pub const VOCAL_FORMS: u8 = 32;

impl VocalForm {
    /// A pronounceable two-syllable rendering, for people reading logs.
    pub fn name(self) -> String {
        const CONSONANTS: [char; 8] = ['k', 't', 'm', 'n', 'r', 's', 'v', 'l'];
        const VOWELS: [char; 4] = ['a', 'i', 'u', 'o'];
        let id = usize::from(self.0);
        let first = CONSONANTS[id % 8];
        let vowel = VOWELS[(id / 8) % 4];
        let second = CONSONANTS[(id * 5 + 3) % 8];
        let last = VOWELS[(id * 3 + 1) % 4];
        format!("{first}{vowel}{second}{last}")
    }
}

/// Entries per lexicon.
pub const LEXICON_SLOTS: usize = 16;
/// Evidence a founder's inherited association starts with.
const INHERITED_EVIDENCE: u16 = 6;
/// Founders come from families of this many (agent ids `0..8`, `8..16`, ...).
pub const FAMILY_SIZE: u32 = 8;
/// Percent of concepts for which a family other than the first has its own word.
const FAMILY_DIALECT_PERCENT: u64 = 33;
/// Percent of founder concepts linked to an idiosyncratic form.
const VARIANT_PERCENT: u64 = 5;
/// Percent of founder concepts with an extra synonym.
const SYNONYM_PERCENT: u64 = 6;

/// One `form → concept` hypothesis. Exactly 12 bytes; `heard == 0` and
/// `positive == 0` marks an empty slot.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
struct LexicalEntry {
    form: u8,
    concept: u8,
    /// Times heard together with evidence for this concept.
    positive: u16,
    /// Times heard together with evidence for another concept.
    contradictory: u16,
    heard: u16,
    /// Times this agent said the form for the concept and it seemed to work / fail (M5).
    successes: u16,
    failures: u16,
}

impl LexicalEntry {
    const fn is_empty(self) -> bool {
        self.positive == 0 && self.heard == 0
    }

    /// Belief strength for recognition: evidence for minus evidence against.
    fn strength(self) -> i32 {
        i32::from(self.positive) - i32::from(self.contradictory)
    }

    /// Preference for production: belief plus experience using it.
    fn production_score(self) -> i32 {
        self.strength() + 2 * i32::from(self.successes) - 2 * i32::from(self.failures)
    }
}

/// A read-only copy of one lexicon entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LexiconEntryView {
    pub form: VocalForm,
    pub concept: Concept,
    pub positive: u16,
    pub contradictory: u16,
    pub heard: u16,
    /// Times this agent said it for the concept and it seemed to work / fail.
    pub successes: u16,
    pub failures: u16,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct Lexicon {
    entries: [LexicalEntry; LEXICON_SLOTS],
}

fn mix(mut key: u64) -> u64 {
    key = (key ^ (key >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    key = (key ^ (key >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    key ^ (key >> 31)
}

/// The founding community's conventional form for each concept (a seed-
/// dependent assignment of distinct forms). Used only to seed founders.
pub(crate) fn founding_form(seed: u64, concept: Concept) -> VocalForm {
    // Fisher–Yates over the forms, keyed by the seed, then take the concept's slot.
    let mut forms: [u8; VOCAL_FORMS as usize] = std::array::from_fn(|index| index as u8);
    let mut key = mix(seed ^ 0x4c45_5849_434f_4e00);
    for index in (1..forms.len()).rev() {
        key = mix(key.wrapping_add(index as u64));
        forms.swap(index, (key % (index as u64 + 1)) as usize);
    }
    VocalForm(forms[concept as usize])
}

/// The word a founding family uses for a concept: the community's word, except
/// that each family after the first has its own word for about a third of the
/// concepts (a dialect).
pub(crate) fn family_form(seed: u64, family: u32, concept: Concept) -> VocalForm {
    if family == 0 {
        return founding_form(seed, concept);
    }
    let roll = mix(seed ^ 0x4641_4d49_4c59 ^ (u64::from(family) << 32) ^ concept as u64);
    if roll % 100 < FAMILY_DIALECT_PERCENT {
        founding_form(
            seed ^ u64::from(family).wrapping_mul(0x9e37_79b9_7f4a_7c15),
            concept,
        )
    } else {
        founding_form(seed, concept)
    }
}

impl Lexicon {
    /// A founder's inherited lexicon: its family's dialect of the community
    /// convention, with a few idiosyncratic forms and occasional synonyms.
    pub(crate) fn founding(seed: u64, agent: AgentId) -> Self {
        let mut lexicon = Self::default();
        let family = agent.get() / FAMILY_SIZE;
        for concept in Concept::ALL {
            let roll = mix(seed ^ (u64::from(agent.get()) << 20) ^ (concept as u64) << 4);
            let conventional = family_form(seed, family, concept);
            let form = if roll % 100 < VARIANT_PERCENT {
                VocalForm(((roll >> 8) % u64::from(VOCAL_FORMS)) as u8)
            } else {
                conventional
            };
            lexicon.inherit(form, concept);
            if (roll >> 24) % 100 < SYNONYM_PERCENT {
                lexicon.inherit(
                    VocalForm(((roll >> 32) % u64::from(VOCAL_FORMS)) as u8),
                    concept,
                );
            }
        }
        lexicon
    }

    fn inherit(&mut self, form: VocalForm, concept: Concept) {
        if self.find(form, concept).is_some() {
            return;
        }
        if let Some(slot) = self.free_slot() {
            self.entries[slot] = LexicalEntry {
                form: form.0,
                concept: concept as u8,
                positive: INHERITED_EVIDENCE,
                ..LexicalEntry::default()
            };
        }
    }

    fn find(&self, form: VocalForm, concept: Concept) -> Option<usize> {
        self.entries.iter().position(|entry| {
            !entry.is_empty() && entry.form == form.0 && entry.concept == concept as u8
        })
    }

    /// An empty slot, else the weakest entry.
    fn free_slot(&self) -> Option<usize> {
        (0..LEXICON_SLOTS)
            .find(|&slot| self.entries[slot].is_empty())
            .or_else(|| {
                (0..LEXICON_SLOTS).min_by_key(|&slot| (self.entries[slot].strength(), slot))
            })
    }

    /// The form this agent would say for `concept`, if it has one.
    pub(crate) fn produce(&self, concept: Concept) -> Option<VocalForm> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| !entry.is_empty() && entry.concept == concept as u8)
            .filter(|(_, entry)| entry.production_score() > 0)
            .max_by_key(|&(slot, entry)| (entry.production_score(), usize::MAX - slot))
            .map(|(_, entry)| VocalForm(entry.form))
    }

    /// What this agent thinks `form` means, if anything (its strongest reading).
    #[cfg(test)]
    pub(crate) fn recognize(&self, form: VocalForm) -> Option<Concept> {
        self.recognize_with_strength(form)
            .map(|(concept, _)| concept)
    }

    /// The strongest reading of `form` and how firmly it is held.
    pub(crate) fn recognize_with_strength(&self, form: VocalForm) -> Option<(Concept, i32)> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| !entry.is_empty() && entry.form == form.0)
            .filter(|(_, entry)| entry.strength() > 0)
            .max_by_key(|&(slot, entry)| (entry.strength(), usize::MAX - slot))
            .map(|(_, entry)| (Concept::from_index(entry.concept), entry.strength()))
    }

    /// Learns from hearing `form` while other evidence pointed to `concept`
    /// (here: an understood mime). Strengthens that link and counts against
    /// other readings of the form. Returns whether the reading was new.
    pub(crate) fn hear_with_evidence(&mut self, form: VocalForm, concept: Concept) -> bool {
        for entry in &mut self.entries {
            if !entry.is_empty() && entry.form == form.0 && entry.concept != concept as u8 {
                entry.contradictory = entry.contradictory.saturating_add(1);
            }
        }
        match self.find(form, concept) {
            Some(slot) => {
                let entry = &mut self.entries[slot];
                entry.positive = entry.positive.saturating_add(1);
                entry.heard = entry.heard.saturating_add(1);
                false
            }
            None => {
                if let Some(slot) = self.free_slot() {
                    self.entries[slot] = LexicalEntry {
                        form: form.0,
                        concept: concept as u8,
                        positive: 1,
                        heard: 1,
                        ..LexicalEntry::default()
                    };
                }
                true
            }
        }
    }

    /// Adds `weight` evidence that `form` means `concept` (from a consequence,
    /// a confirmation, or a repair).
    pub(crate) fn reinforce(&mut self, form: VocalForm, concept: Concept, weight: u16) {
        match self.find(form, concept) {
            Some(slot) => {
                let entry = &mut self.entries[slot];
                entry.positive = entry.positive.saturating_add(weight);
            }
            None => {
                if let Some(slot) = self.free_slot() {
                    self.entries[slot] = LexicalEntry {
                        form: form.0,
                        concept: concept as u8,
                        positive: weight,
                        ..LexicalEntry::default()
                    };
                }
            }
        }
    }

    /// Adds `weight` evidence that `form` does *not* mean `concept`.
    pub(crate) fn contradict(&mut self, form: VocalForm, concept: Concept, weight: u16) {
        if let Some(slot) = self.find(form, concept) {
            let entry = &mut self.entries[slot];
            entry.contradictory = entry.contradictory.saturating_add(weight);
        }
    }

    /// Records whether saying `form` for `concept` seemed to work.
    pub(crate) fn record_use(&mut self, form: VocalForm, concept: Concept, worked: bool) {
        if let Some(slot) = self.find(form, concept) {
            let entry = &mut self.entries[slot];
            if worked {
                entry.successes = entry.successes.saturating_add(1);
            } else {
                entry.failures = entry.failures.saturating_add(1);
            }
        }
    }

    pub(crate) fn views(&self) -> impl Iterator<Item = LexiconEntryView> + '_ {
        self.entries
            .iter()
            .filter(|entry| !entry.is_empty())
            .map(|entry| LexiconEntryView {
                form: VocalForm(entry.form),
                concept: Concept::from_index(entry.concept),
                positive: entry.positive,
                contradictory: entry.contradictory,
                heard: entry.heard,
                successes: entry.successes,
                failures: entry.failures,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexicon_layout_is_compact() {
        assert_eq!(size_of::<LexicalEntry>(), 12);
        assert_eq!(size_of::<Lexicon>(), 12 * LEXICON_SLOTS);
    }

    #[test]
    fn the_founding_convention_gives_each_concept_a_distinct_form() {
        let forms: std::collections::BTreeSet<_> = Concept::ALL
            .iter()
            .map(|&concept| founding_form(7, concept))
            .collect();
        assert_eq!(forms.len(), Concept::COUNT);
        assert_ne!(
            founding_form(7, Concept::Water),
            founding_form(8, Concept::Water)
        );
    }

    #[test]
    fn the_second_family_speaks_a_dialect() {
        let seed = 1;
        let differing = Concept::ALL
            .iter()
            .filter(|&&concept| family_form(seed, 1, concept) != family_form(seed, 0, concept))
            .count();
        assert!(
            differing > 0 && differing < Concept::COUNT,
            "{differing} concepts differ"
        );
    }

    #[test]
    fn founders_mostly_share_the_convention_but_not_entirely() {
        let seed = 1;
        let (mut agree, mut disagree) = (0, 0);
        for id in 0..FAMILY_SIZE {
            let lexicon = Lexicon::founding(seed, AgentId::new(id));
            for concept in Concept::ALL {
                if lexicon.produce(concept) == Some(founding_form(seed, concept)) {
                    agree += 1;
                } else {
                    disagree += 1;
                }
            }
        }
        assert!(agree > disagree * 5, "mostly shared: {agree} vs {disagree}");
        assert!(disagree > 0, "but with individual variation");
    }

    #[test]
    fn hearing_a_form_with_clear_evidence_teaches_it() {
        let mut lexicon = Lexicon::default();
        let form = VocalForm(9);
        assert_eq!(lexicon.recognize(form), None);
        assert!(lexicon.hear_with_evidence(form, Concept::Water));
        assert_eq!(lexicon.recognize(form), Some(Concept::Water));
        // Repeated contrary evidence overturns the reading.
        for _ in 0..3 {
            lexicon.hear_with_evidence(form, Concept::Food);
        }
        assert_eq!(lexicon.recognize(form), Some(Concept::Food));
    }

    #[test]
    fn failed_uses_steer_production_toward_another_word() {
        let mut lexicon = Lexicon::default();
        lexicon.reinforce(VocalForm(1), Concept::Food, 6);
        lexicon.reinforce(VocalForm(2), Concept::Food, 3);
        assert_eq!(lexicon.produce(Concept::Food), Some(VocalForm(1)));
        for _ in 0..2 {
            lexicon.record_use(VocalForm(1), Concept::Food, false);
        }
        assert_eq!(
            lexicon.produce(Concept::Food),
            Some(VocalForm(2)),
            "switches to the word that works"
        );
    }

    #[test]
    fn consequences_can_relearn_a_word() {
        let mut lexicon = Lexicon::default();
        lexicon.reinforce(VocalForm(4), Concept::Water, 6);
        lexicon.contradict(VocalForm(4), Concept::Water, 8);
        lexicon.reinforce(VocalForm(4), Concept::Food, 4);
        assert_eq!(lexicon.recognize(VocalForm(4)), Some(Concept::Food));
    }

    #[test]
    fn forms_have_readable_names() {
        assert_eq!(VocalForm(0).name().len(), 4);
        assert_ne!(VocalForm(1).name(), VocalForm(2).name());
    }
}
