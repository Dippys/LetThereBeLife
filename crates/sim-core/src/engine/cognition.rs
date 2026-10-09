//! Engine side of cognition: updating mental maps from perception, memory-driven
//! deliberation, pointing gestures and who sees them, and read-only belief views.

use super::errors::{move_failure, perception_failure};
use crate::cognition::{
    DesiredEffect, Personality, PublicSignal, Understanding, UtteranceIntent, belief_seconds,
    express, told_confidence, understand,
};
use crate::policy::{MindInput, PolicyAction, PolicySelection, deliberate};
use crate::{
    AgentActivity, AgentId, Engine, ExplorationHeading, GestureTopic, HintOutcomeEvent,
    InterpretationEvent, InventoryView, LandmarkKind, MentalMapView, PHYSICAL_POLICY_RADIUS,
    PhysicalGoal, PhysicalNeedsView, PhysicalPerception, PolicyDiagnostic, PolicyDiagnosticKind,
    PolicyFailureReason, PolicyOptions, PolicyReason, SIGNAL_TICKS, SignalEvent, WorldPosition,
};

/// What delivering one public signal did.
pub(super) struct Delivery {
    pub(super) reading: Understanding,
    pub(super) informed: u16,
    pub(super) watchers: u16,
}

const fn can_watch(activity: AgentActivity) -> bool {
    !matches!(
        activity,
        AgentActivity::Sleeping | AgentActivity::Incapacitated | AgentActivity::Dead
    )
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
        let hint_outcomes = &mut self.hint_outcomes;
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
            ..
        } = mind;
        map.observe(
            agent.get(),
            origin,
            perception,
            now,
            &mut |teller, confirmed, kind| {
                hint_outcomes.push(HintOutcomeEvent {
                    agent,
                    teller: people.agent_in(teller),
                    kind,
                    confirmed,
                    at,
                });
                if social {
                    people.hint_checked(teller, confirmed);
                }
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
        let share = (self.policy_options.sharing && company && map.share_ready(now, cooldown))
            .then(|| map.shareable(perception.area))
            .flatten();
        // Visit friends only when alone; sociable people remember them for longer.
        let friend_target = (social && !company)
            .then(|| {
                people.friend_to_visit(
                    now,
                    Personality::scale(personality.sociability, 300, 1_800) as u32,
                )
            })
            .flatten();
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
            },
        );
        if deliberation.selection.goal == PhysicalGoal::Signal
            && let Some((_, rank)) = share
        {
            self.minds.get_mut(agent).map.mark_shared(now, rank);
        }
        (deliberation.selection, deliberation.heading)
    }

    /// Begins a pointing gesture toward a remembered place.
    pub(super) fn start_signal(
        &mut self,
        agent: AgentId,
        place: WorldPosition,
        reason: PolicyReason,
    ) {
        self.population.clear_route(agent);
        match self.population.schedule_policy_action(
            &mut self.scheduler,
            self.time,
            agent,
            PolicyAction {
                goal: PhysicalGoal::Signal,
                target: place,
                reason,
                duration: SIGNAL_TICKS,
            },
        ) {
            Ok(_) => self.policy_diagnostics.push(PolicyDiagnostic {
                agent,
                at: self.time,
                goal: PhysicalGoal::Signal,
                target: Some(place),
                reason,
                kind: PolicyDiagnosticKind::ActionStarted,
                failure: None,
            }),
            Err(error) => self.schedule_policy_retry(
                agent,
                PhysicalGoal::Signal,
                Some(place),
                reason,
                move_failure(error),
            ),
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
        let topic = self
            .minds
            .get(sender)
            .and_then(|mind| {
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
        let public = express(sender, from, intent, vocal, urgency)
            .ok_or(PolicyFailureReason::TargetUnavailable)?;
        let id = self.next_signal_id;
        self.next_signal_id += 1;
        let delivery = self.deliver(id, &public)?;
        self.signal_events.push(SignalEvent {
            id,
            at: self.time,
            intent,
            signal: public,
            inferred_position: delivery.reading.estimate,
            search_radius: u16::from(delivery.reading.uncertainty) * 4,
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
        let reading = understand(public);
        let perception = self
            .perceive_physical(public.sender, PHYSICAL_POLICY_RADIUS)
            .map_err(perception_failure)?;
        let now = belief_seconds(self.time);
        let (mut watchers, mut informed) = (0_u16, 0_u16);
        let social = self.policy_options.social;
        for watcher in &perception.agents {
            if watcher.id == public.sender || !can_watch(watcher.activity) {
                continue;
            }
            watchers = watchers.saturating_add(1);
            let mind = self.minds.get_mut(watcher.id);
            // Words are learned from what accompanies them (here the understood mime).
            let word_reading = public.vocal.and_then(|form| mind.lexicon.recognize(form));
            if let Some(form) = public.vocal {
                mind.lexicon
                    .hear_with_evidence(form, reading.topic.concept());
            }
            let mut confidence = 0;
            let changed = match reading.topic {
                GestureTopic::Explored => mind.map.record_visit(reading.estimate),
                GestureTopic::Place(kind) => {
                    let teller = if social {
                        mind.notice(public.sender, public.origin, now)
                    } else {
                        None
                    };
                    let trust = teller.map_or(crate::DEFAULT_TRUST, |slot| mind.social.trust(slot));
                    confidence = told_confidence(trust);
                    mind.map.remember_told(
                        kind,
                        reading.estimate,
                        reading.uncertainty,
                        now,
                        teller,
                        confidence,
                    )
                }
            };
            if changed {
                informed = informed.saturating_add(1);
            }
            self.interpretation_events.push(InterpretationEvent {
                signal: id,
                receiver: watcher.id,
                at: self.time,
                understood: reading.topic,
                estimate: reading.estimate,
                search_radius: u16::from(reading.uncertainty) * 4,
                confidence,
                changed,
                heard: public.vocal,
                word_reading,
            });
        }
        Ok(Delivery {
            reading,
            informed,
            watchers,
        })
    }

    /// How urgent the agent looks: its most pressing need relative to that
    /// need's threshold (128 = at threshold, 255 = twice past it or more).
    fn visible_urgency(&self, agent: AgentId) -> u8 {
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
            acquaintances: mind.social.views().collect(),
            lexicon: mind.lexicon.views().collect(),
        })
    }

    /// The agent's innate personality (independent of whether the policy uses it).
    pub fn personality(&self, agent: AgentId) -> Option<Personality> {
        self.population
            .view(agent)
            .map(|_| Personality::of(self.config.seed, agent))
    }

    /// The personality the policy acts on: innate with `social`, else average.
    fn personality_in_use(&self, agent: AgentId) -> Personality {
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

    /// Hints confirmed or abandoned during the latest tick.
    pub fn hint_outcomes(&self) -> &[HintOutcomeEvent] {
        &self.hint_outcomes
    }
}
