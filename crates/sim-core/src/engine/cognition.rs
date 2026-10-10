//! Engine side of cognition: updating mental maps from perception, memory-driven
//! deliberation, pointing gestures and who sees them, and read-only belief views.

use super::errors::{move_failure, perception_failure};
use crate::agent::CompactPosition;
use crate::cognition::{
    CONSEQUENCE_WEIGHT, Concept, DesiredEffect, HintCheck, HintSource, LEAD_SECONDS, Lead,
    LessonCause, LessonEvent, ListenerContext, PendingCorrection, Personality, PublicSignal,
    REPAIR_WEIGHT, RepairEvent, RepairResponse, UtteranceIntent, VocalForm, belief_seconds,
    express, locate, mime_for, spent_kinds, told_confidence, understand, unmistakable,
    visible_kinds,
};
use crate::policy::{
    FoodValues, MindInput, ParentInput, PolicyAction, PolicySelection, deliberate,
};
use crate::{
    AgentActivity, AgentId, Engine, ExplorationHeading, GestureTopic, HintOutcomeEvent,
    InterpretationEvent, InventoryView, LandmarkKind, MentalMapView, PHYSICAL_POLICY_RADIUS,
    PhysicalGoal, PhysicalNeedsView, PhysicalPerception, PolicyDiagnostic, PolicyDiagnosticKind,
    PolicyFailureReason, PolicyOptions, PolicyReason, SIGNAL_TICKS, SignalEvent, WorldPosition,
};

/// What delivering one public signal did.
pub(super) struct Delivery {
    pub(super) estimate: WorldPosition,
    pub(super) uncertainty: u8,
    pub(super) informed: u16,
    pub(super) watchers: u16,
    /// Unsure listeners who mimed their guess back ("this?").
    pub(super) questions: Vec<Question>,
}

/// A listener's visible "this?": its guess mimed back with its own word for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Question {
    pub(super) listener: AgentId,
    pub(super) guess: Concept,
    pub(super) listener_word: Option<VocalForm>,
    pub(super) estimate: WorldPosition,
    pub(super) uncertainty: u8,
}

/// A runner-up reading at least this likely (out of 255) is kept when the
/// listener urgently needs what it would mean.
const RUNNER_UP_MIN_PROBABILITY: u8 = 64;
/// Relative need (128 = at threshold) that counts as urgent for that rule.
const URGENT_NEED: u16 = 112;
/// Agents run from animals they fear once they come this close (cells).
const FLEE_DISTANCE: u64 = 7;
/// An animal in view: its species and where it is.
type SightedAnimal = (crate::Species, WorldPosition);

/// Animals closer than this (cells) can be told apart.
const IDENTIFY_DISTANCE: u64 = 6;
/// A warned-about spot this close (cells) is worth running from.
const ALARM_DISTANCE: u64 = 20;
/// Ticks spent warming up by a hearth.
const WARM_UP_TICKS: u64 = 120;
/// Ticks a warning takes.
const WARNING_TICKS: u64 = 40;
/// How far a shouted warning or call to hunt carries (cells).
const CALL_RADIUS: u8 = 16;
/// Children ask "this?" unless at least this sure (out of 255, about 90%).
const CHILD_ASKS_BELOW: u8 = 230;

/// Hint confidence: trust in the teller, scaled by how sure the reading is.
fn scaled_confidence(trust: u8, probability: u8) -> u8 {
    (u16::from(told_confidence(trust)) * u16::from(probability) / 255) as u8
}

pub(super) const fn can_watch(activity: AgentActivity) -> bool {
    !matches!(
        activity,
        AgentActivity::Sleeping | AgentActivity::Incapacitated | AgentActivity::Dead
    )
}

/// Who learns from a consequence and what it can see.
struct ConsequenceContext {
    agent: AgentId,
    at: crate::SimTime,
    now: u32,
    visible: [bool; LandmarkKind::COUNT],
    teller: Option<AgentId>,
    /// The next gesture id, to recover full ids from a hint's low 16 bits.
    latest_signal: u64,
}

/// Recovers the full id of a recent gesture from its low 16 bits.
fn full_signal_id(low: u16, next: u64) -> u64 {
    let candidate = (next & !0xFFFF) | u64::from(low);
    if candidate >= next {
        candidate.saturating_sub(0x1_0000)
    } else {
        candidate
    }
}

/// A checked hint tests the word it came with. Confirmed: the word meant what the
/// listener thought. Abandoned while the alternative it had weighed is in view:
/// the word probably meant that, and the speaker should hear about it.
fn learn_from_consequence(
    context: ConsequenceContext,
    check: HintCheck,
    lexicon: &mut crate::cognition::Lexicon,
    dialogue: &mut crate::cognition::Dialogue,
    lessons: &mut Vec<LessonEvent>,
) {
    let Some(form) = check.form else {
        return;
    };
    let believed = GestureTopic::Place(check.kind).concept();
    let signal = Some(full_signal_id(check.signal, context.latest_signal));
    if check.confirmed {
        lexicon.reinforce(form, believed, CONSEQUENCE_WEIGHT);
        lessons.push(LessonEvent {
            agent: context.agent,
            at: context.at,
            form,
            strengthened: Some(believed),
            weakened: None,
            use_worked: None,
            cause: LessonCause::Consequence,
            signal,
        });
        return;
    }
    // Only an alternative the listener had actually weighed counts: a stale tip
    // (the berries were eaten) teaches nothing about the word.
    let Some(actual) = check.alternative.filter(|alternative| {
        crate::cognition::concept_topic(*alternative).is_some_and(|topic| match topic {
            GestureTopic::Place(kind) => context.visible[kind as usize],
            GestureTopic::Explored | GestureTopic::Animal(_) => false,
        })
    }) else {
        return;
    };
    lexicon.contradict(form, believed, CONSEQUENCE_WEIGHT);
    lexicon.reinforce(form, actual, CONSEQUENCE_WEIGHT);
    if let Some(speaker) = context.teller {
        dialogue.plan_correction(PendingCorrection {
            speaker,
            form,
            misread: believed,
            actual,
            place: check.place,
            since: context.now,
        });
    }
    lessons.push(LessonEvent {
        agent: context.agent,
        at: context.at,
        form,
        strengthened: Some(actual),
        weakened: Some(believed),
        use_worked: None,
        cause: LessonCause::Consequence,
        signal,
    });
}

impl Engine {
    /// Remembers what `agent` currently sees, then chooses using its mental map.
    pub(super) fn deliberate_with_memory(
        &mut self,
        agent: AgentId,
        origin: WorldPosition,
        needs: PhysicalNeedsView,
        inventory: InventoryView,
        perception: &PhysicalPerception,
    ) -> (PolicySelection, Option<ExplorationHeading>) {
        let now = belief_seconds(self.time);
        let heading = self
            .population
            .exploration_heading(agent)
            .unwrap_or(ExplorationHeading::North);
        let social = self.policy_options.social;
        let personality = self.personality_in_use(agent);
        let at = self.time;
        let latest_signal = self.next_signal_id;
        let beg = self.beg_target(agent, perception);
        let food = self.food_values(agent);
        self.check_leads(agent, perception, now);
        let (seen_danger, seen_prey, wary) = self.animals_of_interest(agent, origin, perception);
        // Young children don't hunt.
        let hunts = self.age_of(agent) >= crate::HUNTING_AGE;
        let (alarm, quarry) = self.minds.get_mut(agent).dialogue.current_leads(now);
        let near = |place: WorldPosition, within: u64| {
            origin.x.abs_diff(place.x).max(origin.y.abs_diff(place.y)) <= within
        };
        // A warned-about spot nearby is a danger even out of sight; a tip about
        // something to hunt is worth following when nothing better is in view.
        let alarm_place = alarm
            .map(|lead| lead.place())
            .filter(|place| near(*place, ALARM_DISTANCE));
        let danger = seen_danger.or(alarm_place);
        let quarry_place = quarry
            .map(|lead| lead.place())
            .filter(|_| seen_prey.is_none());
        let (warn, recruit) = self.signals_worth_making(agent, perception, now);
        let visible = visible_kinds(origin, perception);
        let spent = spent_kinds(perception);
        let hint_outcomes = &mut self.hint_outcomes;
        let lessons = &mut self.lesson_events;
        let mind = self.minds.get_mut(agent);
        if social {
            for other in &perception.agents {
                if other.id != agent && other.activity != AgentActivity::Dead {
                    mind.notice(other.id, other.position, now);
                }
            }
            mind.social.update_whereabouts(perception.area, |id| {
                perception.agents.iter().any(|other| other.id.get() == id)
            });
        }
        let crate::cognition::Mind {
            map,
            social: people,
            lexicon,
            dialogue,
            affordances: _,
            fauna: _,
            crafts,
            child: _,
            parent,
        } = mind;
        map.observe(
            agent.get(),
            origin,
            perception,
            now,
            &mut |check: HintCheck| {
                let teller = check.teller.and_then(|slot| people.agent_in(slot));
                // Stripped bushes in view explain an empty spot: the tip was right
                // but stale, which says nothing about the teller or the word.
                if !check.confirmed && spent[check.kind as usize] {
                    hint_outcomes.push(HintOutcomeEvent {
                        agent,
                        teller,
                        kind: check.kind,
                        confirmed: false,
                        at,
                    });
                    return;
                }
                hint_outcomes.push(HintOutcomeEvent {
                    agent,
                    teller,
                    kind: check.kind,
                    confirmed: check.confirmed,
                    at,
                });
                if social && let Some(slot) = check.teller {
                    people.hint_checked(slot, check.confirmed);
                }
                learn_from_consequence(
                    ConsequenceContext {
                        agent,
                        at,
                        now,
                        visible,
                        teller,
                        latest_signal,
                    },
                    check,
                    lexicon,
                    dialogue,
                    lessons,
                );
            },
        );
        let search_target = if map.seen_count(LandmarkKind::Water) == 0 {
            Some(map.search_target(origin, perception.area))
        } else {
            map.end_search();
            None
        };
        let company = perception
            .agents
            .iter()
            .any(|other| other.id != agent && can_watch(other.activity));
        let cooldown = Personality::scale(personality.sociability, 110, 10) as u32;
        // Setting the record straight with a speaker in view comes before sharing.
        let correction = self
            .policy_options
            .sharing
            .then(|| dialogue.correction(now))
            .flatten()
            .filter(|correction| {
                perception
                    .agents
                    .iter()
                    .any(|other| other.id == correction.speaker && can_watch(other.activity))
            });
        // A correction for someone out of sight: go to where they were last seen.
        let seek = (self.policy_options.sharing && correction.is_none())
            .then(|| dialogue.correction(now))
            .flatten()
            .and_then(|pending| people.whereabouts(pending.speaker));
        let share = correction
            .map(|correction| (correction.place, u8::MAX))
            .or_else(|| {
                (self.policy_options.sharing && company && map.share_ready(now, cooldown))
                    .then(|| map.shareable(perception.area))
                    .flatten()
            });
        // Visit friends only when alone; sociable people remember them for longer.
        let friend_target = (social && !company)
            .then(|| {
                people.friend_to_visit(
                    now,
                    Personality::scale(personality.sociability, 300, 1_800) as u32,
                )
            })
            .flatten();
        let parent = parent.map(|parent| {
            if perception.agents.iter().any(|other| other.id == parent) {
                ParentInput::Stay
            } else {
                people
                    .whereabouts(parent)
                    .map_or(ParentInput::Stay, ParentInput::Return)
            }
        });
        let deliberation = deliberate(
            origin,
            needs,
            inventory,
            perception,
            MindInput {
                map,
                heading,
                share_target: share.map(|(place, _)| place),
                search_target,
                personality,
                company,
                friend_target,
                seek,
                parent,
                beg_target: beg.map(|(_, position)| position),
                food,
                danger,
                wary,
                prey: seen_prey.filter(|_| hunts),
                quarry: quarry_place.filter(|_| hunts),
                warn: warn.map(|(_, position)| position),
                recruit: recruit.map(|(_, position)| position).filter(|_| hunts),
                knows_hearths: crafts.knows_hearths(),
                came_from: map.came_from(origin),
            },
        );
        self.minds.get_mut(agent).map.mark_decision(origin);
        match deliberation.selection.reason {
            PolicyReason::Warning | PolicyReason::Recruiting => {
                let warning = deliberation.selection.reason == PolicyReason::Warning;
                if let Some((species, position)) = if warning { warn } else { recruit } {
                    self.minds
                        .get_mut(agent)
                        .dialogue
                        .plan_animal(species, position, warning, now);
                }
            }
            PolicyReason::Fleeing => {
                if let Some(lead) = alarm {
                    self.follow_lead(agent, lead, true);
                }
            }
            PolicyReason::Hunting if seen_prey.is_none() => {
                if let Some(lead) = quarry {
                    self.follow_lead(agent, lead, false);
                }
            }
            _ => {}
        }
        if deliberation.selection.reason == PolicyReason::Begging
            && let Some((giver, position)) = beg
        {
            self.minds
                .get_mut(agent)
                .dialogue
                .plan_request(giver, position);
        } else if deliberation.selection.goal == PhysicalGoal::Signal
            && let Some((_, rank)) = share
        {
            self.minds.get_mut(agent).map.mark_shared(now, rank);
        }
        (deliberation.selection, deliberation.heading)
    }

    /// Begins an action done where the agent stands (a gesture toward a place,
    /// or a strike at an animal next to it) that completes after its duration.
    pub(super) fn start_timed_action(
        &mut self,
        agent: AgentId,
        goal: PhysicalGoal,
        target: WorldPosition,
        reason: PolicyReason,
    ) {
        let duration = match (goal, reason) {
            (PhysicalGoal::Hunt, _) => super::HUNT_TICKS,
            (PhysicalGoal::WarmUp, _) => WARM_UP_TICKS,
            // A warning is quick: a shout and a point.
            (_, PolicyReason::Warning) => WARNING_TICKS,
            _ => SIGNAL_TICKS,
        };
        self.population.clear_route(agent);
        match self.population.schedule_policy_action(
            &mut self.scheduler,
            self.time,
            agent,
            PolicyAction {
                goal,
                target,
                reason,
                duration,
            },
        ) {
            Ok(_) => self.policy_diagnostics.push(PolicyDiagnostic {
                agent,
                at: self.time,
                goal,
                target: Some(target),
                reason,
                kind: PolicyDiagnosticKind::ActionStarted,
                failure: None,
            }),
            Err(error) => {
                self.schedule_policy_retry(agent, goal, Some(target), reason, move_failure(error));
            }
        }
    }

    /// Completes a gesture. The sender turns its private intent into a public
    /// signal; `deliver` then hands watchers that public signal and nothing else.
    pub(super) fn apply_signal(
        &mut self,
        sender: AgentId,
        place: WorldPosition,
    ) -> Result<(), PolicyFailureReason> {
        let from = self
            .population
            .view(sender)
            .ok_or(PolicyFailureReason::InconsistentState)?
            .position;
        let now = belief_seconds(self.time);
        if let Some(giver) = self.minds.get_mut(sender).dialogue.take_request(place) {
            return self.apply_request(sender, from, giver);
        }
        let correction = self
            .minds
            .get(sender)
            .and_then(|mind| mind.dialogue.correction(now))
            .filter(|correction| correction.place == place);
        if let Some(correction) = correction {
            return self.apply_correction(sender, from, correction);
        }
        let animal = self
            .minds
            .get_mut(sender)
            .dialogue
            .take_animal(place)
            .map(GestureTopic::Animal);
        let topic = animal
            .or_else(|| {
                let mind = self.minds.get(sender)?;
                mind.map
                    .seen_kind_at(place)
                    .map(GestureTopic::Place)
                    .or_else(|| {
                        mind.map
                            .is_explored_marker(place)
                            .then_some(GestureTopic::Explored)
                    })
            })
            .ok_or(PolicyFailureReason::TargetUnavailable)?;
        let intent = UtteranceIntent {
            effect: DesiredEffect::Inform,
            topic,
            place,
        };
        let urgency = self.visible_urgency(sender);
        let vocal = self
            .minds
            .get(sender)
            .and_then(|mind| mind.lexicon.produce(topic.concept()));
        let mime = self.mime_of(sender, topic);
        let public = express(sender, from, intent, mime, vocal, urgency)
            .ok_or(PolicyFailureReason::TargetUnavailable)?;
        let id = self.next_signal_id;
        self.next_signal_id += 1;
        let delivery = self.deliver(id, &public)?;
        for question in &delivery.questions {
            self.answer_question(id, &public, intent, *question, now);
        }
        self.signal_events.push(SignalEvent {
            id,
            at: self.time,
            intent,
            signal: public,
            inferred_position: delivery.estimate,
            search_radius: u16::from(delivery.uncertainty) * 4,
            informed: delivery.informed,
            watchers: delivery.watchers,
        });
        Ok(())
    }

    /// Shows a public signal to every awake agent in view of its sender. This is
    /// the receiver side: it has no access to the sender's intent or memory.
    pub(super) fn deliver(
        &mut self,
        id: u64,
        public: &PublicSignal,
    ) -> Result<Delivery, PolicyFailureReason> {
        let (estimate, uncertainty) = locate(public);
        let bearing = crate::policy::heading_toward(public.origin, estimate);
        let mut questions = Vec::new();
        let reach = if public.loud {
            CALL_RADIUS
        } else {
            PHYSICAL_POLICY_RADIUS
        };
        let perception = self
            .perceive_physical(public.sender, reach)
            .map_err(perception_failure)?;
        let now = belief_seconds(self.time);
        let (mut watchers, mut informed) = (0_u16, 0_u16);
        let social = self.policy_options.social;
        for watcher in &perception.agents {
            if watcher.id == public.sender || !can_watch(watcher.activity) {
                continue;
            }
            watchers = watchers.saturating_add(1);
            let (thirst, hunger) = self.relative_need(watcher.id);
            // Cautious or sociable listeners ask "this?" sooner; bold, reserved ones
            // just go. Average traits ask below about 58% (147 of 255); the range is 45–70%.
            let personality = self.personality_in_use(watcher.id);
            let ask_below = Personality::scale(
                ((u16::from(personality.caution) + u16::from(personality.sociability)) / 2) as u8,
                115,
                179,
            ) as u8;
            let mind = self.minds.get_mut(watcher.id);
            // Children ask about almost anything they aren't sure of.
            let ask_below = if mind.child {
                CHILD_ASKS_BELOW
            } else {
                ask_below
            };
            let word = public
                .vocal
                .and_then(|form| mind.lexicon.recognize_with_strength(form));
            let radius = u64::from(uncertainty) * 4;
            let listener = ListenerContext {
                word,
                heard_word: public.vocal.is_some(),
                thirst,
                hunger,
                remembered_near: LandmarkKind::ALL
                    .map(|kind| mind.map.remembers_near(kind, estimate, radius)),
                food: mind.affordances.food_values(),
            };
            let understanding = understand(public, listener);
            // Words are learned from the listener's own reading, right or wrong.
            if let Some(form) = public.vocal {
                let heard_as = understanding.topic.concept();
                mind.lexicon.hear_with_evidence(form, heard_as);
                if let Some((prior, _)) = word
                    && prior != heard_as
                {
                    self.lesson_events.push(LessonEvent {
                        agent: watcher.id,
                        at: self.time,
                        form,
                        strengthened: Some(heard_as),
                        weakened: Some(prior),
                        use_worked: None,
                        cause: LessonCause::Usage,
                        signal: Some(id),
                    });
                }
            }
            let teller = match (social, understanding.topic) {
                (true, GestureTopic::Place(_)) => mind.notice(public.sender, public.origin, now),
                _ => None,
            };
            let trust = teller.map_or(crate::DEFAULT_TRUST, |slot| mind.social.trust(slot));
            let (best, best_probability) = understanding.reading.best();
            let mut confidence = 0;
            let mut changed = match understanding.topic {
                GestureTopic::Explored => mind.map.record_visit(estimate),
                GestureTopic::Place(kind) => {
                    confidence = scaled_confidence(trust, best_probability);
                    let source = HintSource {
                        teller,
                        form: public.vocal,
                        alternative: understanding
                            .reading
                            .runner_up()
                            .map(|(concept, _)| concept),
                        signal: id,
                        bearing: Some(bearing),
                    };
                    mind.map
                        .remember_told(kind, estimate, uncertainty, now, source, confidence)
                }
                GestureTopic::Animal(species) => {
                    // With no idea about the animal it read, the mime says
                    // what kind of animal it is: a snarl warns, a spear says hunt.
                    let fauna = &mut mind.fauna;
                    if !fauna.dangerous(species) && !fauna.prey(species) {
                        match public.mime {
                            crate::Mime::Snarl => fauna.heard_of_danger(species),
                            crate::Mime::Spear => fauna.saw_hunted(species),
                            _ => {}
                        }
                    }
                    let lead = CompactPosition::checked(estimate).map(|place| Lead {
                        signal: id,
                        speaker: public.sender,
                        until: now + LEAD_SECONDS,
                        place,
                        species,
                        form: public.vocal,
                        warned: public.mime == crate::Mime::Snarl,
                    });
                    if mind.fauna.dangerous(species) {
                        mind.dialogue.alarm = lead;
                    } else if mind.fauna.prey(species) {
                        mind.dialogue.quarry = lead;
                    }
                    lead.is_some()
                }
            };
            // Stakes: a likely-enough reading of something urgently needed is kept too.
            if let Some((runner_up, probability)) = understanding.reading.runner_up()
                && runner_up != best
                && probability >= RUNNER_UP_MIN_PROBABILITY
                && let Some(GestureTopic::Place(kind)) = crate::cognition::concept_topic(runner_up)
                && ((kind == LandmarkKind::Water && thirst >= URGENT_NEED)
                    || (kind == LandmarkKind::Berries && hunger >= URGENT_NEED))
            {
                let source = HintSource {
                    teller,
                    form: public.vocal,
                    alternative: Some(best),
                    signal: id,
                    bearing: Some(bearing),
                };
                changed |= mind.map.remember_told(
                    kind,
                    estimate,
                    uncertainty,
                    now,
                    source,
                    scaled_confidence(trust, probability),
                );
            }
            if let (Some(negated), Some(form)) = (public.negated, public.vocal) {
                let wrong = unmistakable(negated);
                let right = unmistakable(public.mime);
                let speaks_it = mind.lexicon.produce(right) == Some(form);
                let (strengthened, weakened) = if speaks_it {
                    // Its own word was taken the wrong way. As after a failed
                    // round of a naming game, it trusts the word less for what
                    // it meant: others evidently hear it otherwise.
                    mind.lexicon.contradict(form, right, REPAIR_WEIGHT);
                    mind.lexicon.record_use(form, right, false);
                    (None, Some(right))
                } else {
                    mind.lexicon.contradict(form, wrong, REPAIR_WEIGHT);
                    mind.lexicon.reinforce(form, right, REPAIR_WEIGHT);
                    (Some(right), Some(wrong))
                };
                self.lesson_events.push(LessonEvent {
                    agent: watcher.id,
                    at: self.time,
                    form,
                    strengthened,
                    weakened,
                    use_worked: speaks_it.then_some(false),
                    cause: LessonCause::Correction,
                    signal: Some(id),
                });
            } else if matches!(understanding.topic, GestureTopic::Place(_))
                && best_probability < ask_below
            {
                // Unsure listeners mime their guess back with their own word for it.
                questions.push(Question {
                    listener: watcher.id,
                    guess: best,
                    listener_word: mind.lexicon.produce(best),
                    estimate,
                    uncertainty,
                });
            }
            if changed {
                informed = informed.saturating_add(1);
            }
            self.interpretation_events.push(InterpretationEvent {
                signal: id,
                receiver: watcher.id,
                at: self.time,
                understood: understanding.topic,
                estimate,
                search_radius: u16::from(uncertainty) * 4,
                confidence,
                changed,
                heard: public.vocal,
                word_reading: word.map(|(concept, _)| concept),
                reading: understanding.reading,
            });
        }
        Ok(Delivery {
            estimate,
            uncertainty,
            informed,
            watchers,
            questions,
        })
    }

    /// The mime `agent` makes for `topic`, given what it believes about the
    /// place's material.
    pub(super) fn mime_of(&self, agent: AgentId, topic: GestureTopic) -> crate::Mime {
        let food = match topic {
            GestureTopic::Place(kind) => kind
                .material()
                .and_then(|material| self.minds.affordances(agent).food_value(material)),
            GestureTopic::Explored => None,
            // For animals the "value" is whether it's feared or hunted.
            GestureTopic::Animal(species) => {
                let fauna = self.minds.get(agent).map(|mind| mind.fauna);
                fauna.map(|fauna| {
                    if fauna.dangerous(species) {
                        -1
                    } else {
                        i16::from(fauna.prey(species))
                    }
                })
            }
        };
        mime_for(topic, food)
    }

    /// Whether anyone awake is within shouting range of `agent`.
    fn someone_within_call(&self, agent: AgentId) -> bool {
        let Some(origin) = self.population.view(agent).map(|view| view.position) else {
            return false;
        };
        let reach = i64::from(CALL_RADIUS);
        let mut nearby = Vec::new();
        self.population.spatial().agents_in(
            crate::WorldRect {
                min: WorldPosition {
                    x: origin.x - reach,
                    y: origin.y - reach,
                },
                max: WorldPosition {
                    x: origin.x + reach + 1,
                    y: origin.y + reach + 1,
                },
            },
            &mut nearby,
        );
        nearby.into_iter().any(|other| {
            other != agent
                && self
                    .population
                    .view(other)
                    .is_some_and(|view| can_watch(view.activity))
        })
    }

    /// Logs that `agent` acted on a tip (once per tip).
    fn follow_lead(&mut self, agent: AgentId, lead: Lead, fled: bool) {
        let already = self
            .lead_events
            .iter()
            .any(|event| event.agent == agent && event.signal == lead.signal);
        if !already {
            self.lead_events.push(crate::LeadFollowedEvent {
                agent,
                at: self.time,
                signal: lead.signal,
                species: lead.species,
                fled,
            });
        }
    }

    /// A dangerous animal in view worth warning the others about, and an animal
    /// worth calling them to hunt, when someone awake is near and it hasn't
    /// just done so.
    fn signals_worth_making(
        &self,
        agent: AgentId,
        perception: &PhysicalPerception,
        now: u32,
    ) -> (Option<SightedAnimal>, Option<SightedAnimal>) {
        let Some(mind) = self.minds.get(agent) else {
            return (None, None);
        };
        if !self.policy_options.sharing || !self.someone_within_call(agent) {
            return (None, None);
        }
        let first = |wanted: &dyn Fn(crate::Species) -> bool| {
            perception
                .animals
                .iter()
                .filter(|animal| wanted(animal.species))
                .min_by_key(|animal| animal.id)
                .map(|animal| (animal.species, animal.position))
        };
        // Someone who was just warned assumes the others heard it too.
        let warn = (mind.dialogue.may_warn(now) && mind.dialogue.alarm.is_none())
            .then(|| first(&|species| mind.fauna.dangerous(species)))
            .flatten();
        let recruit = mind
            .dialogue
            .may_recruit(now)
            .then(|| first(&|species| mind.fauna.prey(species) && !mind.fauna.dangerous(species)))
            .flatten();
        (warn, recruit)
    }

    /// Checks tips about animals against what's actually there, once the agent
    /// is close enough to tell one animal from another. Finding another animal
    /// where one was pointed out (a wolf where it understood "deer") teaches what
    /// the word must have meant, and it plans to tell the speaker. Finding nothing
    /// once there just ends the tip (animals move).
    fn check_leads(&mut self, agent: AgentId, perception: &PhysicalPerception, now: u32) {
        let origin = self.population.view(agent).map(|view| view.position);
        let at = self.time;
        let mind = self.minds.get_mut(agent);
        for quarry in [false, true] {
            let lead = if quarry {
                mind.dialogue.quarry
            } else {
                mind.dialogue.alarm
            };
            let Some(lead) = lead else {
                continue;
            };
            let place = lead.place();
            let near_place = |position: WorldPosition| {
                position
                    .x
                    .abs_diff(place.x)
                    .max(position.y.abs_diff(place.y))
                    <= 8
            };
            let identifiable = |position: WorldPosition| {
                origin.is_some_and(|origin| {
                    origin
                        .x
                        .abs_diff(position.x)
                        .max(origin.y.abs_diff(position.y))
                        <= IDENTIFY_DISTANCE
                })
            };
            let there: Vec<crate::Species> = perception
                .animals
                .iter()
                .filter(|animal| near_place(animal.position) && identifiable(animal.position))
                .map(|animal| animal.species)
                .collect();
            let clear = |dialogue: &mut crate::cognition::Dialogue| {
                if quarry {
                    dialogue.quarry = None;
                } else {
                    dialogue.alarm = None;
                }
            };
            if there.contains(&lead.species) {
                continue;
            }
            if let Some(&actual) = there.first() {
                // It was warned with a snarl and found this: so this is the
                // dangerous one.
                if lead.warned {
                    mind.fauna.heard_of_danger(actual);
                }
                let misread = GestureTopic::Animal(lead.species).concept();
                let meant = GestureTopic::Animal(actual).concept();
                if let Some(form) = lead.form {
                    mind.lexicon.contradict(form, misread, CONSEQUENCE_WEIGHT);
                    mind.lexicon.reinforce(form, meant, CONSEQUENCE_WEIGHT);
                    self.lesson_events.push(LessonEvent {
                        agent,
                        at,
                        form,
                        strengthened: Some(meant),
                        weakened: Some(misread),
                        use_worked: None,
                        cause: LessonCause::Consequence,
                        signal: Some(lead.signal),
                    });
                    mind.dialogue.plan_correction(PendingCorrection {
                        speaker: lead.speaker,
                        form,
                        misread,
                        actual: meant,
                        place,
                        since: now,
                    });
                }
                clear(&mut mind.dialogue);
            } else if identifiable(place) {
                clear(&mut mind.dialogue);
            }
        }
    }

    /// The nearest animal in view `agent` believes is dangerous (if it's close
    /// enough to worry about), the nearest it believes is worth hunting, and the
    /// nearest dangerous one at any distance (to keep errands away from it).
    fn animals_of_interest(
        &self,
        agent: AgentId,
        origin: WorldPosition,
        perception: &PhysicalPerception,
    ) -> (
        Option<WorldPosition>,
        Option<WorldPosition>,
        Option<WorldPosition>,
    ) {
        let Some(mind) = self.minds.get(agent) else {
            return (None, None, None);
        };
        let distance = |position: WorldPosition| {
            origin
                .x
                .abs_diff(position.x)
                .max(origin.y.abs_diff(position.y))
        };
        let nearest = |wanted: &dyn Fn(crate::Species) -> bool, within: u64| {
            perception
                .animals
                .iter()
                .filter(|animal| wanted(animal.species) && distance(animal.position) <= within)
                .min_by_key(|animal| (distance(animal.position), animal.id))
                .map(|animal| animal.position)
        };
        (
            nearest(&|species| mind.fauna.dangerous(species), FLEE_DISTANCE),
            nearest(
                &|species| mind.fauna.prey(species) && !mind.fauna.dangerous(species),
                u64::MAX,
            ),
            nearest(&|species| mind.fauna.dangerous(species), u64::MAX),
        )
    }

    /// What `agent` wants to eat. With a mind: its beliefs, plus (only while
    /// hungry) a taste of anything it has never tried. Without one (legacy
    /// policy): what an all-knowing agent would eat.
    pub(super) fn food_values(&self, agent: AgentId) -> FoodValues {
        if !self.policy_options.memory {
            return FoodValues::truth();
        }
        let hungry = self
            .population
            .needs_view(agent, self.time)
            .is_ok_and(|needs| needs.hunger.threshold_reached);
        let beliefs = self.minds.affordances(agent).food_values();
        FoodValues(beliefs.map(|value| value.unwrap_or(i16::from(hungry))))
    }

    /// Thirst and hunger relative to their thresholds (128 = at threshold).
    pub(super) fn relative_need(&self, agent: AgentId) -> (u16, u16) {
        self.population
            .needs_view(agent, self.time)
            .map_or((0, 0), |needs| {
                let relative = |level: crate::NeedLevelView| {
                    (u32::from(level.value) * 128 / u32::from(level.threshold.max(1))).min(255)
                        as u16
                };
                (relative(needs.thirst), relative(needs.hunger))
            })
    }

    /// The sender sees a listener's "this?" and, knowing what it meant, nods or
    /// repeats with an exaggerated mime. The listener only sees that answer.
    fn answer_question(
        &mut self,
        id: u64,
        public: &PublicSignal,
        intent: UtteranceIntent,
        question: Question,
        now: u32,
    ) {
        let meant = intent.topic.concept();
        let response = if question.guess == meant {
            RepairResponse::Confirmed
        } else {
            RepairResponse::Repaired(unmistakable(public.mime))
        };
        // Sender side: did its word work, and what does the listener call it?
        let sender = self.minds.get_mut(public.sender);
        if let Some(form) = public.vocal {
            sender
                .lexicon
                .record_use(form, meant, response == RepairResponse::Confirmed);
            self.lesson_events.push(LessonEvent {
                agent: public.sender,
                at: self.time,
                form,
                strengthened: None,
                weakened: None,
                use_worked: Some(response == RepairResponse::Confirmed),
                cause: if response == RepairResponse::Confirmed {
                    LessonCause::Confirmation
                } else {
                    LessonCause::Repair
                },
                signal: Some(id),
            });
        }
        if let Some(listener_word) = question.listener_word
            && response != RepairResponse::Confirmed
        {
            sender
                .lexicon
                .hear_with_evidence(listener_word, question.guess);
        }
        // Listener side: only the public nod or exaggerated mime.
        let listener = self.minds.get_mut(question.listener);
        let (strengthened, weakened) = match response {
            RepairResponse::Confirmed => (question.guess, None),
            RepairResponse::Repaired(shown) => {
                if let Some(GestureTopic::Place(wrong)) =
                    crate::cognition::concept_topic(question.guess)
                {
                    listener.map.forget_hint(wrong, question.estimate);
                }
                if let Some(GestureTopic::Place(right)) = crate::cognition::concept_topic(shown) {
                    let teller = listener.social.slot_of(public.sender);
                    let trust =
                        teller.map_or(crate::DEFAULT_TRUST, |slot| listener.social.trust(slot));
                    let source = HintSource {
                        teller,
                        form: public.vocal,
                        alternative: None,
                        signal: id,
                        bearing: Some(crate::policy::heading_toward(
                            public.origin,
                            question.estimate,
                        )),
                    };
                    listener.map.remember_told(
                        right,
                        question.estimate,
                        question.uncertainty,
                        now,
                        source,
                        told_confidence(trust),
                    );
                }
                (shown, Some(question.guess))
            }
        };
        if let Some(form) = public.vocal {
            listener
                .lexicon
                .reinforce(form, strengthened, REPAIR_WEIGHT);
            if let Some(wrong) = weakened {
                listener.lexicon.contradict(form, wrong, REPAIR_WEIGHT);
            }
            self.lesson_events.push(LessonEvent {
                agent: question.listener,
                at: self.time,
                form,
                strengthened: Some(strengthened),
                weakened,
                use_worked: None,
                cause: if weakened.is_some() {
                    LessonCause::Repair
                } else {
                    LessonCause::Confirmation
                },
                signal: Some(id),
            });
        }
        self.repair_events.push(RepairEvent {
            signal: id,
            listener: question.listener,
            at: self.time,
            guess: question.guess,
            listener_word: question.listener_word,
            response,
        });
    }

    /// "You said that word, but over there was this, not that": points back at
    /// the place, says the word, shows what was really there, and waves away
    /// what it was taken to mean.
    pub(super) fn apply_correction(
        &mut self,
        sender: AgentId,
        from: WorldPosition,
        correction: PendingCorrection,
    ) -> Result<(), PolicyFailureReason> {
        // The speaker may have walked off while this was being prepared: keep the
        // correction in mind for the next meeting instead of telling no one.
        let speaker_watching = self
            .perceive_physical(sender, PHYSICAL_POLICY_RADIUS)
            .map_err(perception_failure)?
            .agents
            .iter()
            .any(|other| other.id == correction.speaker && can_watch(other.activity));
        if !speaker_watching {
            return Err(PolicyFailureReason::TargetUnavailable);
        }
        self.minds.get_mut(sender).dialogue.clear_correction();
        let (Some(actual), Some(misread)) = (
            crate::cognition::concept_topic(correction.actual),
            crate::cognition::concept_topic(correction.misread),
        ) else {
            return Err(PolicyFailureReason::TargetUnavailable);
        };
        let intent = UtteranceIntent {
            effect: DesiredEffect::Correct,
            topic: actual,
            place: correction.place,
        };
        let urgency = self.visible_urgency(sender);
        let mime = self.mime_of(sender, actual);
        let mut public = express(sender, from, intent, mime, Some(correction.form), urgency)
            .ok_or(PolicyFailureReason::TargetUnavailable)?;
        public.negated = Some(self.mime_of(sender, misread));
        let id = self.next_signal_id;
        self.next_signal_id += 1;
        let delivery = self.deliver(id, &public)?;
        self.signal_events.push(SignalEvent {
            id,
            at: self.time,
            intent,
            signal: public,
            inferred_position: delivery.estimate,
            search_radius: u16::from(delivery.uncertainty) * 4,
            informed: delivery.informed,
            watchers: delivery.watchers,
        });
        Ok(())
    }

    /// Word lessons from the latest tick (for logs and tools).
    pub fn lesson_events(&self) -> &[LessonEvent] {
        &self.lesson_events
    }

    /// "This?" questions and their answers from the latest tick (for logs and tools).
    pub fn repair_events(&self) -> &[RepairEvent] {
        &self.repair_events
    }

    /// How urgent the agent looks: its most pressing need relative to that
    /// need's threshold (128 = at threshold, 255 = twice past it or more).
    pub(super) fn visible_urgency(&self, agent: AgentId) -> u8 {
        self.population
            .needs_view(agent, self.time)
            .map_or(0, |needs| {
                [needs.hunger, needs.thirst, needs.rest, needs.exposure]
                    .iter()
                    .map(|level| u32::from(level.value) * 128 / u32::from(level.threshold.max(1)))
                    .max()
                    .unwrap_or(0)
                    .min(255) as u8
            })
    }

    /// The cognitive features the active policy uses.
    pub fn policy_options(&self) -> PolicyOptions {
        self.policy_options
    }

    /// A copy of what `agent` knows about places and people. `None` before it has a mind.
    pub fn mental_map(&self, agent: AgentId) -> Option<MentalMapView> {
        let mind = self.minds.get(agent)?;
        Some(MentalMapView {
            agent,
            personality: self.personality_in_use(agent),
            landmarks: mind.map.views().collect(),
            explored_tiles: mind.map.explored_tile_count(),
            child: mind.child,
            affordances: mind.affordances.views().collect(),
            fauna: mind.fauna.views().collect(),
            knows_hearths: mind.crafts.knows_hearths(),
            acquaintances: mind.social.views().collect(),
            lexicon: mind.lexicon.views().collect(),
        })
    }

    /// Agents with ids at or above `count` are children: they start with no words,
    /// stay close to whoever they are bonded with, and ask readily. Call before
    /// the first tick (minds are created on first use); reset restores "all founders".
    pub fn set_founders(&mut self, count: u32) {
        self.minds.set_founders(count);
    }

    /// Bonds a child to a parent: the child knows and trusts the parent from the start.
    pub fn bond(&mut self, child: AgentId, parent: AgentId) -> bool {
        let Some(position) = self.population.view(parent).map(|view| view.position) else {
            return false;
        };
        let now = belief_seconds(self.time);
        let mind = self.minds.get_mut(child);
        mind.parent = Some(parent);
        mind.social.bond(parent, position, now).is_some()
    }

    /// The agent's innate personality (independent of whether the policy uses it).
    pub fn personality(&self, agent: AgentId) -> Option<Personality> {
        self.population
            .view(agent)
            .map(|_| Personality::of(self.config.seed, agent))
    }

    /// The personality the policy acts on: innate with `social`, else average.
    pub(super) fn personality_in_use(&self, agent: AgentId) -> Personality {
        if self.policy_options.social {
            Personality::of(self.config.seed, agent)
        } else {
            Personality::AVERAGE
        }
    }

    /// Gestures completed during the latest tick (private intent included: tools only).
    pub fn signal_events(&self) -> &[SignalEvent] {
        &self.signal_events
    }

    /// How each watcher read the latest tick's gestures.
    pub fn interpretation_events(&self) -> &[InterpretationEvent] {
        &self.interpretation_events
    }

    /// Tips about animals acted on during the latest tick (for logs and tools).
    pub fn lead_events(&self) -> &[crate::LeadFollowedEvent] {
        &self.lead_events
    }

    /// Meals eaten during the latest tick (for logs and tools).
    pub fn meal_events(&self) -> &[crate::MealEvent] {
        &self.meal_events
    }

    /// Hints confirmed or abandoned during the latest tick.
    pub fn hint_outcomes(&self) -> &[HintOutcomeEvent] {
        &self.hint_outcomes
    }
}
