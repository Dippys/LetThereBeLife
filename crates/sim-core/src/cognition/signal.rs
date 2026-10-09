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
    GestureTopic, LandmarkKind, VocalForm,
    gesture::{Gesture, interpret, point},
    reading::{ListenerContext, Reading, concept_topic, read},
};

/// What a sender wants its signal to achieve. Requests come with M7.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesiredEffect {
    /// "There is something over there."
    Inform,
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
    pub tone: Tone,
}

/// The mime a sender performs for a topic (the sender's production habit).
pub(crate) const fn mime_for(topic: GestureTopic) -> Mime {
    match topic {
        GestureTopic::Place(LandmarkKind::Water) => Mime::Scoop,
        GestureTopic::Place(LandmarkKind::Food) => Mime::PickAndChew,
        GestureTopic::Place(LandmarkKind::Wood) => Mime::Chop,
        GestureTopic::Place(LandmarkKind::Stone) => Mime::Strike,
        GestureTopic::Place(LandmarkKind::Shelter) => Mime::RestHead,
        GestureTopic::Explored => Mime::Sweep,
    }
}

/// Turns a private intent into what others can see. `None` when the place is
/// too close to be worth pointing at.
pub(crate) fn express(
    sender: AgentId,
    origin: WorldPosition,
    intent: UtteranceIntent,
    vocal: Option<VocalForm>,
    urgency: u8,
) -> Option<PublicSignal> {
    Some(PublicSignal {
        sender,
        origin,
        pointing: point(origin, intent.place)?,
        mime: mime_for(intent.topic),
        vocal,
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
            thirst: 0,
            hunger: 0,
            remembered_near: [false; 5],
        }
    }

    #[test]
    fn every_topic_has_a_distinct_mime_that_watchers_read_back() {
        let topics = [
            GestureTopic::Place(LandmarkKind::Water),
            GestureTopic::Place(LandmarkKind::Food),
            GestureTopic::Place(LandmarkKind::Wood),
            GestureTopic::Place(LandmarkKind::Stone),
            GestureTopic::Place(LandmarkKind::Shelter),
            GestureTopic::Explored,
        ];
        for topic in topics {
            let intent = UtteranceIntent {
                effect: DesiredEffect::Inform,
                topic,
                place: at(80, -30),
            };
            let signal = express(AgentId::new(1), at(0, 0), intent, None, 40).unwrap();
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
        let signal = express(AgentId::new(3), at(5, 5), intent, None, 200).unwrap();
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
            topic: GestureTopic::Place(LandmarkKind::Food),
            place: at(3, 3),
        };
        assert_eq!(express(AgentId::new(0), at(0, 0), intent, None, 0), None);
    }
}
