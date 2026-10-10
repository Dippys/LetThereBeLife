//! Interpretation with competing meanings (spec 05 "Interpretation evidence").
//!
//! A listener scores a *bounded* set of candidate concepts using only what it
//! can observe or already knows: the mime (physically ambiguous: scooping and
//! eating both bring a hand to the mouth), its own reading of the spoken word,
//! its own needs, what it already remembers near the indicated place, and the
//! sender's visible urgency. The result is a small probability distribution and
//! a record of why. Nothing here can see the sender's intent.

use super::{Concept, GestureTopic, LandmarkKind, Mime, PublicSignal};
use crate::Material;

/// Candidates kept per reading.
pub const READING_CANDIDATES: usize = 3;

/// Why a reading came out the way it did. Several can apply.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReadingReasons {
    /// The mime looks like more than one thing.
    pub ambiguous_mime: bool,
    /// A word was spoken that this listener doesn't know.
    pub unknown_word: bool,
    /// The listener's reading of the word disagrees with what the mime suggests most.
    pub word_disagrees: bool,
    /// The listener's own need tipped the balance.
    pub need_bias: bool,
    /// What the listener already remembered near that place tipped the balance.
    pub memory_bias: bool,
}

/// One listener's reading of one public signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    /// Best candidates first, with probabilities out of 255 (they sum to at most 255).
    pub candidates: [(Concept, u8); READING_CANDIDATES],
    pub candidate_count: u8,
    pub reasons: ReadingReasons,
}

impl Reading {
    pub fn best(&self) -> (Concept, u8) {
        self.candidates[0]
    }

    pub fn runner_up(&self) -> Option<(Concept, u8)> {
        (self.candidate_count > 1).then_some(self.candidates[1])
    }
}

/// What the listener brings to a signal: its own knowledge and state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ListenerContext {
    /// The listener's own reading of the spoken word and how strongly it holds it.
    pub(crate) word: Option<(Concept, i32)>,
    /// A word was heard (whether or not the listener knows it).
    pub(crate) heard_word: bool,
    /// Thirst and hunger relative to their thresholds (128 = at threshold).
    pub(crate) thirst: u16,
    pub(crate) hunger: u16,
    /// Place kinds the listener already remembers near the indicated spot.
    pub(crate) remembered_near: [bool; LandmarkKind::COUNT],
    /// What the listener believes each material is worth eating (`None` = no idea).
    pub(crate) food: [Option<i16>; Material::COUNT],
}

impl ListenerContext {
    /// A listener with no word, needs, memories, or food beliefs.
    #[cfg(test)]
    pub(crate) const fn blank() -> Self {
        Self {
            word: None,
            heard_word: false,
            thirst: 0,
            hunger: 0,
            remembered_near: [false; LandmarkKind::COUNT],
            food: [None; Material::COUNT],
        }
    }
}

/// How strongly a mime suggests each concept, strongest first. Mimes are
/// physical movements, so similar movements give similar evidence, and what an
/// eating or retching mime suggests depends on what the listener believes is
/// food or makes you sick.
fn mime_evidence(mime: Mime, listener: &ListenerContext) -> [(Concept, i32); 3] {
    let belief = |material: Material| listener.food[material as usize];
    let eaten = |material: Material| match belief(material) {
        Some(value) if value > 0 => 30,
        Some(value) if value < 0 => 4,
        _ => 14,
    };
    let sickening = |material: Material| match belief(material) {
        Some(value) if value < 0 => 40,
        Some(value) if value > 0 => 4,
        _ => 14,
    };
    let mut evidence = match mime {
        Mime::Scoop => [
            (Concept::Water, 30),
            (Concept::Berries, eaten(Material::Berries) * 2 / 3),
            (
                Concept::Bitterberries,
                eaten(Material::Bitterberries) * 2 / 3,
            ),
        ],
        Mime::PickAndChew => [
            (Concept::Berries, eaten(Material::Berries)),
            (Concept::Bitterberries, eaten(Material::Bitterberries)),
            (Concept::Water, 20),
        ],
        Mime::Retch => [
            (Concept::Bitterberries, sickening(Material::Bitterberries)),
            (Concept::Berries, sickening(Material::Berries)),
            (Concept::Water, 0),
        ],
        Mime::Chop => [
            (Concept::Wood, 40),
            (Concept::Stone, 10),
            (Concept::Home, 0),
        ],
        Mime::Strike => [
            (Concept::Stone, 40),
            (Concept::Wood, 10),
            (Concept::Home, 0),
        ],
        Mime::RestHead => [(Concept::Home, 40), (Concept::Been, 0), (Concept::Wood, 0)],
        Mime::Sweep => [(Concept::Been, 40), (Concept::Home, 0), (Concept::Wood, 0)],
    };
    evidence.sort_by_key(|&(concept, weight)| (-weight, concept));
    evidence
}

/// Evidence a known word adds: more for well-established readings.
fn word_weight(strength: i32) -> i32 {
    15 + strength.clamp(0, 30)
}

/// Evidence a pressing need adds to the concept that would satisfy it.
fn need_weight(relative: u16) -> i32 {
    if relative < 64 {
        0
    } else {
        (i32::from(relative.min(255)) - 64) * 15 / 191
    }
}

const MEMORY_WEIGHT: i32 = 12;
const URGENCY_WEIGHT: i32 = 5;

pub(crate) const fn concept_kind(concept: Concept) -> Option<LandmarkKind> {
    match concept {
        Concept::Water => Some(LandmarkKind::Water),
        Concept::Berries => Some(LandmarkKind::Berries),
        Concept::Wood => Some(LandmarkKind::Wood),
        Concept::Stone => Some(LandmarkKind::Stone),
        Concept::Home => Some(LandmarkKind::Shelter),
        Concept::Bitterberries => Some(LandmarkKind::Bitterberries),
        _ => None,
    }
}

/// The topic a concept refers to, if gestures can be about it.
pub const fn concept_topic(concept: Concept) -> Option<GestureTopic> {
    match concept {
        Concept::Been => Some(GestureTopic::Explored),
        _ => match concept_kind(concept) {
            Some(kind) => Some(GestureTopic::Place(kind)),
            None => None,
        },
    }
}

/// Scores the candidates. Deterministic: ties break by concept order.
pub(crate) fn read(signal: &PublicSignal, listener: ListenerContext) -> Reading {
    let mut scores = [0_i32; Concept::COUNT];
    let mime = mime_evidence(signal.mime, &listener);
    for (concept, weight) in mime {
        scores[concept as usize] += weight;
    }
    if let Some((concept, strength)) = listener.word {
        scores[concept as usize] += word_weight(strength);
    }
    let thirst = need_weight(listener.thirst);
    let hunger = need_weight(listener.hunger);
    scores[Concept::Water as usize] += thirst;
    // Hunger favors whatever the listener thinks is food.
    for (material, concept) in [
        (Material::Berries, Concept::Berries),
        (Material::Bitterberries, Concept::Bitterberries),
    ] {
        if listener.food[material as usize].is_some_and(|value| value > 0) {
            scores[concept as usize] += hunger;
        }
    }
    let mut memory_scores = [0_i32; Concept::COUNT];
    for concept in Concept::ALL {
        if let Some(kind) = concept_kind(concept)
            && listener.remembered_near[kind as usize]
            && scores[concept as usize] > 0
        {
            scores[concept as usize] += MEMORY_WEIGHT;
            memory_scores[concept as usize] = MEMORY_WEIGHT;
        }
    }
    if signal.tone.urgency >= 128 {
        scores[Concept::Water as usize] += URGENCY_WEIGHT;
        scores[Concept::Berries as usize] += URGENCY_WEIGHT;
    }

    // Only gesture-able concepts with positive evidence are candidates.
    let mut ranked: Vec<(Concept, i32)> = Concept::ALL
        .into_iter()
        .filter(|concept| concept_topic(*concept).is_some() && scores[*concept as usize] > 0)
        .map(|concept| (concept, scores[concept as usize]))
        .collect();
    ranked.sort_by_key(|&(concept, score)| (-score, concept));
    ranked.truncate(READING_CANDIDATES);
    let total: i32 = ranked.iter().map(|&(_, score)| score).sum::<i32>().max(1);
    let mut candidates = [(Concept::Water, 0_u8); READING_CANDIDATES];
    for (slot, &(concept, score)) in ranked.iter().enumerate() {
        candidates[slot] = (concept, (score * 255 / total) as u8);
    }

    let best = ranked.first().map(|&(concept, _)| concept);
    let mime_favorite = mime[0].0;
    let without = |extra: &[i32; Concept::COUNT]| {
        // Would the winner change without this evidence?
        let mut alternative = scores;
        for (score, removed) in alternative.iter_mut().zip(extra) {
            *score -= removed;
        }
        Concept::ALL
            .into_iter()
            .filter(|concept| concept_topic(*concept).is_some())
            .max_by_key(|concept| (alternative[*concept as usize], -(*concept as i32)))
    };
    let mut need_scores = [0_i32; Concept::COUNT];
    need_scores[Concept::Water as usize] = thirst;
    for (material, concept) in [
        (Material::Berries, Concept::Berries),
        (Material::Bitterberries, Concept::Bitterberries),
    ] {
        if listener.food[material as usize].is_some_and(|value| value > 0) {
            need_scores[concept as usize] = hunger;
        }
    }
    let reasons = ReadingReasons {
        ambiguous_mime: mime[1].1 > 0 && mime[1].1 * 2 >= mime[0].1,
        unknown_word: listener.heard_word && listener.word.is_none(),
        word_disagrees: listener
            .word
            .is_some_and(|(concept, _)| concept != mime_favorite),
        need_bias: (thirst > 0 || hunger > 0) && without(&need_scores) != best,
        memory_bias: memory_scores.iter().any(|&score| score > 0)
            && without(&memory_scores) != best,
    };
    Reading {
        candidates,
        candidate_count: ranked.len() as u8,
        reasons,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AgentId, WorldPosition,
        cognition::{Tone, VocalForm, gesture::point},
    };

    fn signal(mime: Mime, urgency: u8) -> PublicSignal {
        PublicSignal {
            sender: AgentId::new(0),
            origin: WorldPosition { x: 0, y: 0 },
            pointing: point(WorldPosition { x: 0, y: 0 }, WorldPosition { x: 60, y: 0 }).unwrap(),
            mime,
            vocal: Some(VocalForm(3)),
            negated: None,
            addressee: None,
            tone: Tone { urgency },
        }
    }

    fn listener(word: Option<(Concept, i32)>, thirst: u16, hunger: u16) -> ListenerContext {
        let mut food = [None; Material::COUNT];
        food[Material::Berries as usize] = Some(118);
        food[Material::Stone as usize] = Some(0);
        ListenerContext {
            word,
            heard_word: true,
            thirst,
            hunger,
            food,
            ..ListenerContext::blank()
        }
    }

    #[test]
    fn a_known_word_and_a_matching_mime_read_clearly() {
        let reading = read(
            &signal(Mime::Scoop, 0),
            listener(Some((Concept::Water, 20)), 0, 0),
        );
        assert_eq!(reading.best().0, Concept::Water);
        assert!(
            reading.best().1 > 150,
            "confident: {:?}",
            reading.candidates
        );
        assert_eq!(reading.runner_up().map(|(c, _)| c), Some(Concept::Berries));
        assert!(reading.reasons.ambiguous_mime);
        assert!(!reading.reasons.need_bias);
    }

    #[test]
    fn a_hungry_listener_who_doesnt_know_the_word_reads_scooping_as_food() {
        let reading = read(&signal(Mime::Scoop, 0), listener(None, 0, 255));
        assert_eq!(reading.best().0, Concept::Berries);
        assert!(reading.reasons.unknown_word);
        assert!(reading.reasons.need_bias);
        assert!(reading.reasons.ambiguous_mime);
    }

    #[test]
    fn a_misheld_word_can_override_an_ambiguous_mime() {
        let reading = read(
            &signal(Mime::Scoop, 0),
            listener(Some((Concept::Berries, 10)), 0, 0),
        );
        assert_eq!(reading.best().0, Concept::Berries);
        assert!(reading.reasons.word_disagrees);
    }

    #[test]
    fn distinct_mimes_resist_a_contrary_word() {
        let reading = read(
            &signal(Mime::Chop, 0),
            listener(Some((Concept::Water, 0)), 0, 0),
        );
        assert_eq!(reading.best().0, Concept::Wood);
    }

    #[test]
    fn memory_near_the_place_can_tip_an_ambiguous_reading() {
        let mut context = listener(None, 0, 0);
        context.remembered_near[LandmarkKind::Berries as usize] = true;
        let reading = read(&signal(Mime::Scoop, 0), context);
        assert_eq!(reading.best().0, Concept::Berries);
        assert!(reading.reasons.memory_bias);
    }

    #[test]
    fn probabilities_are_bounded_and_ordered() {
        let reading = read(&signal(Mime::PickAndChew, 200), listener(None, 128, 128));
        let total: u32 = reading.candidates[..usize::from(reading.candidate_count)]
            .iter()
            .map(|&(_, p)| u32::from(p))
            .sum();
        assert!(total <= 255);
        assert!(reading.candidates[0].1 >= reading.candidates[1].1);
    }
}
