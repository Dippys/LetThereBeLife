//! Interpretation with competing meanings (spec 05 "Interpretation evidence").
//!
//! A listener scores a *bounded* set of candidate concepts using only what it
//! can observe or already knows: the mime (physically ambiguous: scooping and
//! eating both bring a hand to the mouth), its own reading of the spoken word,
//! its own needs, what it already remembers near the indicated place, and the
//! sender's visible urgency. The result is a small probability distribution and
//! a record of why. Nothing here can see the sender's intent.

use super::{
    Concept, GestureTopic, LandmarkKind, Mime, PublicSignal,
    signal::{handling_motion, natural_motion},
};
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

/// The motions a listener associates with a concept, with how strongly. What
/// eating and retching suggest depends on what the listener believes is food or
/// makes you sick; things picked by hand look like they might be eaten.
fn associations(concept: Concept, listener: &ListenerContext) -> [(Mime, i32); 2] {
    match concept {
        Concept::Material(material) => {
            let properties = material.properties();
            let looks_edible = matches!(
                properties.handling,
                crate::Handling::Pick | crate::Handling::Carve
            );
            match listener.food[material as usize] {
                Some(value) if value > 0 => [(Mime::PickAndChew, 30), (Mime::Retch, 4)],
                Some(value) if value < 0 => [(Mime::Retch, 40), (Mime::PickAndChew, 4)],
                _ if looks_edible => [(Mime::PickAndChew, 14), (Mime::Retch, 14)],
                _ => [(handling_motion(properties.handling), 40), (Mime::Sweep, 0)],
            }
        }
        Concept::Water => [(Mime::Scoop, 30), (Mime::Sweep, 0)],
        Concept::Species(_) => [(natural_motion(concept), 30), (Mime::Sweep, 0)],
        _ => [(natural_motion(concept), 40), (Mime::Sweep, 0)],
    }
}

/// How alike two motions look, out of 12: scooping water and eating both bring
/// a hand to the mouth; chopping and striking are both blows; a snarl and a
/// raised spear are both tense postures aimed at an animal.
const fn likeness(seen: Mime, associated: Mime) -> i32 {
    if seen as u8 == associated as u8 {
        return 12;
    }
    match (seen, associated) {
        (Mime::Scoop, Mime::PickAndChew) | (Mime::PickAndChew, Mime::Scoop) => 8,
        (Mime::Snarl, Mime::Spear) | (Mime::Spear, Mime::Snarl) => 7,
        (Mime::Chop, Mime::Strike)
        | (Mime::Strike, Mime::Chop)
        | (Mime::Warm, Mime::RestHead)
        | (Mime::RestHead, Mime::Warm) => 3,
        _ => 0,
    }
}

/// How strongly a mime suggests each concept, strongest first (the top three).
fn mime_evidence(mime: Mime, listener: &ListenerContext) -> [(Concept, i32); READING_CANDIDATES] {
    let mut evidence: Vec<(Concept, i32)> = Concept::ALL
        .into_iter()
        .map(|concept| {
            let weight = associations(concept, listener)
                .into_iter()
                .map(|(motion, strength)| strength * likeness(mime, motion) / 12)
                .max()
                .unwrap_or(0);
            (concept, weight)
        })
        .collect();
    evidence.sort_by_key(|&(concept, weight)| (-weight, concept));
    let mut top = [(Concept::Water, 0); READING_CANDIDATES];
    top.copy_from_slice(&evidence[..READING_CANDIDATES]);
    top
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
        Concept::Material(material) => LandmarkKind::of_material(material),
        Concept::Structure(kind) => Some(LandmarkKind::Structure(kind)),
        Concept::Been | Concept::Species(_) => None,
    }
}

/// The topic a concept refers to, if gestures can be about it.
pub const fn concept_topic(concept: Concept) -> Option<GestureTopic> {
    match concept {
        Concept::Been => Some(GestureTopic::Explored),
        Concept::Species(species) => Some(GestureTopic::Animal(species)),
        _ => match concept_kind(concept) {
            Some(kind) => Some(GestureTopic::Place(kind)),
            None => None,
        },
    }
}

/// Animals with some evidence that don't bite: what a hungry listener would
/// go after.
fn huntable(scores: &[i32; Concept::COUNT]) -> Vec<Concept> {
    crate::Species::ALL
        .into_iter()
        .map(Concept::Species)
        .filter(|concept| scores[concept.index()] > 0)
        .filter(|concept| matches!(natural_motion(*concept), Mime::Spear))
        .collect()
}

/// Materials the listener believes are worth eating.
fn believed_food(listener: &ListenerContext) -> Vec<Concept> {
    Material::ALL
        .into_iter()
        .filter(|material| listener.food[*material as usize].is_some_and(|value| value > 0))
        .map(Concept::Material)
        .collect()
}

/// Scores the candidates. Deterministic: ties break by concept order.
pub(crate) fn read(signal: &PublicSignal, listener: ListenerContext) -> Reading {
    // A thing held up is plain to see, whatever the word or mime.
    if let Some(material) = signal.shown {
        let mut candidates = [(Concept::Water, 0); READING_CANDIDATES];
        candidates[0] = (Concept::Material(material), u8::MAX);
        return Reading {
            candidates,
            candidate_count: 1,
            reasons: ReadingReasons::default(),
        };
    }
    let mut scores = [0_i32; Concept::COUNT];
    let mime = mime_evidence(signal.mime, &listener);
    for (concept, weight) in mime {
        scores[concept.index()] += weight;
    }
    if let Some((concept, strength)) = listener.word {
        scores[concept.index()] += word_weight(strength);
    }
    let thirst = need_weight(listener.thirst);
    let hunger = need_weight(listener.hunger);
    scores[Concept::Water.index()] += thirst;
    // A hungry listener hears a call about a harmless animal as a call to hunt,
    // and favors whatever it thinks is food.
    let hunted = huntable(&scores);
    for concept in &hunted {
        scores[concept.index()] += hunger;
    }
    let food = believed_food(&listener);
    for concept in &food {
        scores[concept.index()] += hunger;
    }
    let mut memory_scores = [0_i32; Concept::COUNT];
    for concept in Concept::ALL {
        if let Some(kind) = concept_kind(concept)
            && listener.remembered_near[kind.index()]
            && scores[concept.index()] > 0
        {
            scores[concept.index()] += MEMORY_WEIGHT;
            memory_scores[concept.index()] = MEMORY_WEIGHT;
        }
    }
    // Urgency suggests something needed: water, or what the listener thinks is food.
    if signal.tone.urgency >= 128 {
        scores[Concept::Water.index()] += URGENCY_WEIGHT;
        for concept in &food {
            scores[concept.index()] += URGENCY_WEIGHT;
        }
    }

    // Only gesture-able concepts with positive evidence are candidates.
    let mut ranked: Vec<(Concept, i32)> = Concept::ALL
        .into_iter()
        .filter(|concept| concept_topic(*concept).is_some() && scores[concept.index()] > 0)
        .map(|concept| (concept, scores[concept.index()]))
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
            .max_by_key(|concept| (alternative[concept.index()], -(concept.index() as i32)))
    };
    let mut need_scores = [0_i32; Concept::COUNT];
    need_scores[Concept::Water.index()] = thirst;
    for concept in hunted.iter().chain(&food) {
        need_scores[concept.index()] = hunger;
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
            shown: None,
            addressee: None,
            loud: false,
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
        assert_eq!(reading.runner_up().map(|(c, _)| c), Some(Concept::BERRIES));
        assert!(reading.reasons.ambiguous_mime);
        assert!(!reading.reasons.need_bias);
    }

    #[test]
    fn a_hungry_listener_who_doesnt_know_the_word_reads_scooping_as_food() {
        let reading = read(&signal(Mime::Scoop, 0), listener(None, 0, 255));
        assert_eq!(reading.best().0, Concept::BERRIES);
        assert!(reading.reasons.unknown_word);
        assert!(reading.reasons.need_bias);
        assert!(reading.reasons.ambiguous_mime);
    }

    #[test]
    fn a_thing_held_up_outweighs_any_word_or_mime() {
        let mut shown = signal(Mime::PickAndChew, 0);
        shown.shown = Some(Material::Berries);
        let reading = read(&shown, listener(Some((Concept::BITTERBERRIES, 30)), 0, 0));
        assert_eq!(reading.best(), (Concept::BERRIES, u8::MAX));
        assert_eq!(reading.candidate_count, 1);
    }

    #[test]
    fn a_misheld_word_can_override_an_ambiguous_mime() {
        let reading = read(
            &signal(Mime::Scoop, 0),
            listener(Some((Concept::BERRIES, 10)), 0, 0),
        );
        assert_eq!(reading.best().0, Concept::BERRIES);
        assert!(reading.reasons.word_disagrees);
    }

    #[test]
    fn distinct_mimes_resist_a_contrary_word() {
        let reading = read(
            &signal(Mime::Chop, 0),
            listener(Some((Concept::Water, 0)), 0, 0),
        );
        assert_eq!(reading.best().0, Concept::WOOD);
    }

    #[test]
    fn memory_near_the_place_can_tip_an_ambiguous_reading() {
        let mut context = listener(None, 0, 0);
        context.remembered_near[LandmarkKind::BERRIES.index()] = true;
        let reading = read(&signal(Mime::Scoop, 0), context);
        assert_eq!(reading.best().0, Concept::BERRIES);
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
