//! Requests for food (spec 05's "request" desired effect; plan M7). A hungry
//! agent holds out an open hand to someone in view, mimes eating, and says its
//! word for food. The one asked reads that like any other signal, so a request
//! can be misread; seeing the wrong guess mimed back, the asker repeats itself
//! with an exaggerated mime. The one asked then gives or refuses depending on
//! its own food and hunger, trust and familiarity, sociability, the asker's
//! visible urgency, and whether the asker is its child. Only the answer (food
//! handed over, a head shake, or empty hands) is visible to the asker.

use super::cognition::can_watch;
use super::errors::perception_failure;
use crate::cognition::{
    Concept, DesiredEffect, LessonCause, LessonEvent, ListenerContext, Mime, PublicSignal,
    REPAIR_WEIGHT, RepairEvent, RepairResponse, RequestEvent, RequestResponse, Tone,
    UtteranceIntent, belief_seconds, reach_toward, understand,
};
use crate::{
    AgentId, DEFAULT_TRUST, Engine, FOOD_CONSUMPTION, GestureTopic, InterpretationEvent,
    LandmarkKind, PHYSICAL_POLICY_RADIUS, PhysicalPerception, PolicyFailureReason, SignalEvent,
    WorldPosition,
};

/// Food handed over per request: one meal.
const GIFT: u8 = FOOD_CONSUMPTION;
/// Willingness (see `decide_gift`) needed to share with someone who isn't one's child.
const GIVE_THRESHOLD: i32 = 260;
/// Relative hunger (128 = at threshold) above which even a parent keeps its food.
const PARENT_KEEPS_FOOD_ABOVE: u16 = 200;

impl Engine {
    /// Someone in view worth asking for food, offered when `agent` is hungry,
    /// carries none, and hasn't asked too recently. Prefers its parent, then the
    /// most trusted and familiar.
    pub(super) fn beg_target(
        &self,
        agent: AgentId,
        perception: &PhysicalPerception,
    ) -> Option<(AgentId, WorldPosition)> {
        if !self.policy_options.helping {
            return None;
        }
        let mind = self.minds.get(agent);
        if mind.is_some_and(|mind| !mind.dialogue.may_request(belief_seconds(self.time)))
            || self
                .food_values(agent)
                .carried(self.population.inventory(agent)?)
                > 0
        {
            return None;
        }
        perception
            .agents
            .iter()
            .filter(|other| other.id != agent && can_watch(other.activity))
            .max_by_key(|other| {
                let (parent, trust, familiarity) = mind.map_or((false, 0, 0), |mind| {
                    let slot = mind.social.slot_of(other.id);
                    (
                        mind.parent == Some(other.id),
                        slot.map_or(0, |slot| mind.social.trust(slot)),
                        slot.map_or(0, |slot| mind.social.familiarity(slot)),
                    )
                });
                (parent, trust, familiarity, u32::MAX - other.id.get())
            })
            .map(|other| (other.id, other.position))
    }

    /// Asks `giver` for food (completing a `Begging` gesture).
    pub(super) fn apply_request(
        &mut self,
        asker: AgentId,
        from: WorldPosition,
        giver: AgentId,
    ) -> Result<(), PolicyFailureReason> {
        let now = belief_seconds(self.time);
        let giver_position = self
            .perceive_physical(asker, PHYSICAL_POLICY_RADIUS)
            .map_err(perception_failure)?
            .agents
            .iter()
            .find(|other| other.id == giver && can_watch(other.activity))
            .map(|other| other.position)
            .ok_or(PolicyFailureReason::TargetUnavailable)?;
        let intent = UtteranceIntent {
            effect: DesiredEffect::Request,
            topic: GestureTopic::Place(LandmarkKind::Berries),
            place: giver_position,
        };
        let urgency = self.visible_urgency(asker);
        let vocal = self
            .minds
            .get(asker)
            .and_then(|mind| mind.lexicon.produce(Concept::Berries));
        let public = PublicSignal {
            sender: asker,
            origin: from,
            pointing: reach_toward(from, giver_position)
                .ok_or(PolicyFailureReason::TargetUnavailable)?,
            mime: Mime::PickAndChew,
            vocal,
            negated: None,
            addressee: Some(giver),
            loud: false,
            tone: Tone { urgency },
        };
        let id = self.next_signal_id;
        self.next_signal_id += 1;

        // The one asked reads the request with its own words and needs.
        let (thirst, hunger) = self.relative_need(giver);
        let mind = self.minds.get_mut(giver);
        let word = vocal.and_then(|form| mind.lexicon.recognize_with_strength(form));
        let understanding = understand(
            &public,
            ListenerContext {
                word,
                heard_word: vocal.is_some(),
                thirst,
                hunger,
                remembered_near: [false; LandmarkKind::COUNT],
                food: mind.affordances.food_values(),
            },
        );
        let read_as = understanding.reading.best().0;
        if let Some(form) = vocal {
            mind.lexicon.hear_with_evidence(form, read_as);
        }
        self.interpretation_events.push(InterpretationEvent {
            signal: id,
            receiver: giver,
            at: self.time,
            understood: understanding.topic,
            estimate: giver_position,
            search_radius: 0,
            confidence: 0,
            changed: false,
            heard: vocal,
            word_reading: word.map(|(concept, _)| concept),
            reading: understanding.reading,
        });
        if read_as == Concept::Berries {
            if let Some(form) = vocal {
                self.minds
                    .get_mut(asker)
                    .lexicon
                    .record_use(form, Concept::Berries, true);
            }
        } else {
            self.repair_request(id, asker, giver, vocal, read_as);
        }

        let response = self.decide_gift(giver, asker, urgency);
        let mut given = None;
        if response == RequestResponse::Gave
            && let Some(material) = self
                .population
                .inventory(giver)
                .and_then(|inventory| self.food_values(giver).best_carried(inventory))
        {
            let taken = self.population.take(giver, material, GIFT);
            let accepted = self.population.add_inventory(asker, material, taken);
            debug_assert_eq!(accepted, taken, "the asker carried no food");
            given = Some(material);
            // Being handed something to eat says the giver thinks it's food.
            self.minds
                .get_mut(asker)
                .affordances
                .saw_eaten(material, false);
        }
        // The asker sees the answer.
        let asker_mind = self.minds.get_mut(asker);
        asker_mind
            .dialogue
            .request_answered(now, response == RequestResponse::Refused);
        if response != RequestResponse::NothingToGive
            && let Some(slot) = asker_mind.notice(giver, giver_position, now)
        {
            asker_mind
                .social
                .helped(slot, response == RequestResponse::Gave);
        }
        self.request_events.push(RequestEvent {
            signal: id,
            asker,
            giver,
            at: self.time,
            read_as,
            response,
            given,
        });
        self.signal_events.push(SignalEvent {
            id,
            at: self.time,
            intent,
            signal: public,
            inferred_position: giver_position,
            search_radius: 0,
            informed: u16::from(response == RequestResponse::Gave),
            watchers: 1,
        });
        Ok(())
    }

    /// The one asked mimed back what it thought was wanted (with its own word
    /// for that); the asker, who knows it meant food, repeats with an
    /// exaggerated mime. Both learn from the exchange.
    fn repair_request(
        &mut self,
        id: u64,
        asker: AgentId,
        giver: AgentId,
        vocal: Option<crate::VocalForm>,
        read_as: Concept,
    ) {
        let giver_mind = self.minds.get_mut(giver);
        let listener_word = giver_mind.lexicon.produce(read_as);
        if let Some(form) = vocal {
            giver_mind.lexicon.contradict(form, read_as, REPAIR_WEIGHT);
            giver_mind
                .lexicon
                .reinforce(form, Concept::Berries, REPAIR_WEIGHT);
            self.lesson_events.push(LessonEvent {
                agent: giver,
                at: self.time,
                form,
                strengthened: Some(Concept::Berries),
                weakened: Some(read_as),
                use_worked: None,
                cause: LessonCause::Repair,
                signal: Some(id),
            });
        }
        let asker_mind = self.minds.get_mut(asker);
        if let Some(form) = vocal {
            asker_mind.lexicon.record_use(form, Concept::Berries, false);
        }
        if let Some(word) = listener_word {
            asker_mind.lexicon.hear_with_evidence(word, read_as);
        }
        self.repair_events.push(RepairEvent {
            signal: id,
            listener: giver,
            at: self.time,
            guess: read_as,
            listener_word,
            response: RepairResponse::Repaired(Concept::Berries),
        });
    }

    /// Whether `giver` shares a meal with `asker`. Parents feed their children
    /// unless starving themselves; anyone else keeps its last meal and weighs
    /// trust, familiarity, sociability, the asker's visible urgency, its own
    /// food, and its own hunger.
    fn decide_gift(&mut self, giver: AgentId, asker: AgentId, urgency: u8) -> RequestResponse {
        let food = self
            .population
            .inventory(giver)
            .map_or(0, |inventory| self.food_values(giver).carried(inventory));
        if food < GIFT {
            return RequestResponse::NothingToGive;
        }
        let (_, hunger) = self.relative_need(giver);
        let own_child = self
            .minds
            .get(asker)
            .is_some_and(|mind| mind.parent == Some(giver));
        if own_child {
            return if hunger <= PARENT_KEEPS_FOOD_ABOVE {
                RequestResponse::Gave
            } else {
                RequestResponse::Refused
            };
        }
        // Only a parent hands over its last meal.
        if food <= GIFT {
            return RequestResponse::Refused;
        }
        let sociability = self.personality_in_use(giver).sociability;
        let mind = self.minds.get_mut(giver);
        let (trust, familiarity) = mind
            .social
            .slot_of(asker)
            .map_or((DEFAULT_TRUST, 0), |slot| {
                (mind.social.trust(slot), mind.social.familiarity(slot))
            });
        let willingness = i32::from(trust)
            + i32::from(familiarity) / 4
            + i32::from(sociability) / 2
            + i32::from(urgency) / 4
            + i32::from(food.min(8)) * 8
            - i32::from(hunger) / 2;
        if willingness >= GIVE_THRESHOLD {
            RequestResponse::Gave
        } else {
            RequestResponse::Refused
        }
    }

    /// Requests for food from the latest tick (for logs and tools).
    pub fn request_events(&self) -> &[RequestEvent] {
        &self.request_events
    }
}
