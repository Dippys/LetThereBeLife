//! Private intent versus public signal (spec 05: "Private intent and public
//! signal must be separate").
//!
//! A sender forms an [`UtteranceIntent`] that only it knows, and *expresses* it
//! as a [`PublicSignal`]: where it points, a mime it performs, and how urgent it
//! looks. Receivers get nothing else. [`understand`] is the only path from a
//! signal to a belief, and it takes the public signal alone, so no receiver can
//! read the sender's intent even by accident.

use crate::{AgentId, WorldPosition};

use super::{
    Concept, GestureTopic, LandmarkKind, VocalForm,
    gesture::{Gesture, MIN_ANIMAL_POINTING_DISTANCE, interpret, point, point_within},
    reading::{ListenerContext, Reading, concept_topic, read},
};

/// What a sender wants its signal to achieve. Requests come with M7.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesiredEffect {
    /// "There is something over there."
    Inform,
    /// "You said that word, but over there was this, not that."
    Correct,
    /// "Give me some" (an open hand held out to someone).
    Request,
}

/// The sender's private reason for signalling. Never delivered to receivers;
/// kept for logs and, later, for judging whether it was understood.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UtteranceIntent {
    pub effect: DesiredEffect,
    pub topic: GestureTopic,
    /// The exact place the sender has in mind.
    pub place: WorldPosition,
}

/// A visible pantomime that accompanies pointing, the pre-language way to say
/// "what". Mimes are public behavior; what they *mean* is up to the watcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Mime {
    /// Cupped hands scooped to the mouth.
    Scoop,
    /// Picking something and chewing.
    PickAndChew,
    /// A chopping motion.
    Chop,
    /// Striking one hand on the other like stone on stone.
    Strike,
    /// Head resting on folded hands.
    RestHead,
    /// A wide sweep of the arm across the pointed direction.
    Sweep,
    /// Hand on the belly and a grimace: "that makes you sick".
    Retch,
    /// Bared teeth and clawing hands: "something dangerous".
    Snarl,
    /// A crouch and a throwing arm: "something to hunt".
    Spear,
    /// Hands held out and rubbed together: "warmth".
    Warm,
}

/// How the sender looks while signalling, derived from its own state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tone {
    /// 0 = calm, 255 = desperate (its most pressing need relative to its threshold).
    pub urgency: u8,
}

/// Everything a watcher can observe of one signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicSignal {
    pub sender: AgentId,
    pub origin: WorldPosition,
    pub pointing: Gesture,
    pub mime: Mime,
    /// The word the sender says, if it has one for the concept.
    pub vocal: Option<VocalForm>,
    /// For corrections: a mime shown and then waved away ("not this").
    pub negated: Option<Mime>,
    /// For requests: who the open hand is held out to.
    pub addressee: Option<AgentId>,
    /// Shouted (warnings and calls to hunt carry farther than a quiet gesture).
    pub loud: bool,
    pub tone: Tone,
}

/// The mime a sender performs for a topic, given what the sender believes the
/// place's material is worth eating (`food`, its own belief, if any): the eating
/// mime for food, a grimace for something that makes you sick, otherwise the
/// motion of gathering it.
pub(crate) const fn mime_for(topic: GestureTopic, food: Option<i16>) -> Mime {
    match topic {
        GestureTopic::Place(LandmarkKind::Water) => Mime::Scoop,
        GestureTopic::Place(LandmarkKind::Structure(kind)) => purpose_motion(kind.purpose()),
        GestureTopic::Explored => Mime::Sweep,
        GestureTopic::Animal(_) => match food {
            Some(value) if value < 0 => Mime::Snarl,
            _ => Mime::Spear,
        },
        GestureTopic::Place(LandmarkKind::Material(material)) => match food {
            Some(value) if value > 0 => Mime::PickAndChew,
            Some(value) if value < 0 => Mime::Retch,
            _ => handling_motion(material.properties().handling),
        },
    }
}

/// The motion of gathering a material that way.
pub(crate) const fn handling_motion(handling: crate::Handling) -> Mime {
    match handling {
        crate::Handling::Pick | crate::Handling::Carve => Mime::PickAndChew,
        crate::Handling::Chop => Mime::Chop,
        crate::Handling::Strike => Mime::Strike,
    }
}

/// How people show what a structure is for.
pub(crate) const fn purpose_motion(purpose: crate::Purpose) -> Mime {
    match purpose {
        crate::Purpose::Rest => Mime::RestHead,
        crate::Purpose::Warmth => Mime::Warm,
    }
}

/// The motion that depicts a concept by what the thing really is: eating for
/// food, retching for what makes you sick, otherwise how it's gathered or used;
/// a snarl for an animal that bites, a spear for one that doesn't.
pub(crate) const fn natural_motion(concept: Concept) -> Mime {
    match concept {
        Concept::Water => Mime::Scoop,
        Concept::Been => Mime::Sweep,
        Concept::Material(material) => {
            let properties = material.properties();
            if properties.toxicity > 0 {
                Mime::Retch
            } else if properties.nutrition > 0 {
                Mime::PickAndChew
            } else {
                handling_motion(properties.handling)
            }
        }
        Concept::Species(species) => {
            if species.traits().bite > 0 {
                Mime::Snarl
            } else {
                Mime::Spear
            }
        }
        Concept::Structure(kind) => purpose_motion(kind.purpose()),
    }
}

/// Turns a private intent into what others can see. `None` when the place is
/// too close to be worth pointing at.
pub(crate) fn express(
    sender: AgentId,
    origin: WorldPosition,
    intent: UtteranceIntent,
    mime: Mime,
    vocal: Option<VocalForm>,
    urgency: u8,
) -> Option<PublicSignal> {
    // Animals in plain sight, and the spot a correction is about, can be
    // pointed at from close by.
    let pointing = match (intent.topic, intent.effect) {
        (GestureTopic::Animal(_), _) | (_, DesiredEffect::Correct) => {
            point_within(origin, intent.place, MIN_ANIMAL_POINTING_DISTANCE)?
        }
        _ => point(origin, intent.place)?,
    };
    Some(PublicSignal {
        sender,
        origin,
        pointing,
        mime,
        vocal,
        negated: None,
        addressee: None,
        loud: matches!(intent.topic, GestureTopic::Animal(_)),
        tone: Tone { urgency },
    })
}

/// A watcher's reading of a public signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Understanding {
    /// Competing meanings and why.
    pub reading: Reading,
    /// The topic of the best reading.
    pub topic: GestureTopic,
    pub estimate: WorldPosition,
    /// Search radius in 4-cell units (as stored by the mental map).
    pub uncertainty: u8,
}

/// The only path from a signal to a belief: the public signal plus the
/// listener's own context (its lexicon, needs, and memories), nothing else.
pub(crate) fn understand(signal: &PublicSignal, listener: ListenerContext) -> Understanding {
    let reading = read(signal, listener);
    let topic = concept_topic(reading.best().0).unwrap_or(GestureTopic::Explored);
    let (estimate, uncertainty) = interpret(signal.origin, signal.pointing);
    Understanding {
        reading,
        topic,
        estimate,
        uncertainty,
    }
}

/// What an exaggerated (or explicitly negated) mime unmistakably shows.
pub(crate) const fn unmistakable(mime: Mime) -> Concept {
    let mut index = 0;
    while index < Concept::COUNT {
        let concept = Concept::ALL[index];
        if natural_motion(concept) as u8 == mime as u8 {
            return concept;
        }
        index += 1;
    }
    Concept::Water
}

/// Where the pointing leads, for anyone watching.
pub(crate) fn locate(signal: &PublicSignal) -> (WorldPosition, u8) {
    interpret(signal.origin, signal.pointing)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: i64, y: i64) -> WorldPosition {
        WorldPosition { x, y }
    }

    /// A listener who knows the word for `topic` well and needs nothing.
    fn clear_listener(topic: GestureTopic) -> ListenerContext {
        ListenerContext {
            word: Some((topic.concept(), 20)),
            heard_word: true,
            ..ListenerContext::blank()
        }
    }

    #[test]
    fn every_topic_has_a_distinct_mime_that_watchers_read_back() {
        let topics = [
            GestureTopic::Place(LandmarkKind::Water),
            GestureTopic::Place(LandmarkKind::BERRIES),
            GestureTopic::Place(LandmarkKind::WOOD),
            GestureTopic::Place(LandmarkKind::STONE),
            GestureTopic::Place(LandmarkKind::SHELTER),
            GestureTopic::Explored,
        ];
        for topic in topics {
            let intent = UtteranceIntent {
                effect: DesiredEffect::Inform,
                topic,
                place: at(80, -30),
            };
            let signal = express(
                AgentId::new(1),
                at(0, 0),
                intent,
                mime_for(intent.topic, None),
                None,
                40,
            )
            .unwrap();
            assert_eq!(understand(&signal, clear_listener(topic)).topic, topic);
        }
    }

    #[test]
    fn tone_and_position_are_public_but_the_exact_place_is_not() {
        let intent = UtteranceIntent {
            effect: DesiredEffect::Inform,
            topic: GestureTopic::Place(LandmarkKind::Water),
            place: at(137, 41),
        };
        let signal = express(
            AgentId::new(3),
            at(5, 5),
            intent,
            mime_for(intent.topic, None),
            None,
            200,
        )
        .unwrap();
        assert_eq!(signal.tone.urgency, 200);
        let reading = understand(&signal, clear_listener(intent.topic));
        assert_ne!(
            reading.estimate, intent.place,
            "watchers only get an estimate"
        );
        let error = reading
            .estimate
            .x
            .abs_diff(intent.place.x)
            .max(reading.estimate.y.abs_diff(intent.place.y));
        assert!(error <= u64::from(reading.uncertainty) * 4);
    }

    #[test]
    fn nearby_places_produce_no_signal() {
        let intent = UtteranceIntent {
            effect: DesiredEffect::Inform,
            topic: GestureTopic::Place(LandmarkKind::BERRIES),
            place: at(3, 3),
        };
        assert_eq!(
            express(
                AgentId::new(0),
                at(0, 0),
                intent,
                mime_for(intent.topic, None),
                None,
                0
            ),
            None
        );
    }
}
