//! Engine side of cognition: updating mental maps from perception, memory-driven
//! deliberation, pointing gestures and who sees them, and read-only belief views.

use super::errors::{move_failure, perception_failure};
use crate::agent::CompactPosition;
use crate::cognition::{
    CONSEQUENCE_WEIGHT, Concept, DesiredEffect, HintCheck, HintSource, LEAD_SECONDS, Lead,
    LessonCause, LessonEvent, ListenerContext, PendingCorrection, Personality, PublicSignal,
    REPAIR_WEIGHT, RepairEvent, RepairResponse, Tie, UtteranceIntent, VocalForm, belief_seconds,
    express, locate, mime_for, spent_kinds, told_confidence, understand, unmistakable,
    visible_kinds,
};
use crate::policy::{
    FoodValues, MindInput, ParentInput, PolicyAction, PolicySelection, deliberate,
};
use crate::wildlife::mix;
use crate::{
    AgentActivity, AgentId, Engine, ExplorationHeading, GestureTopic, HintOutcomeEvent,
    InterpretationEvent, InventoryView, LandmarkKind, MentalMapView, PHYSICAL_POLICY_RADIUS,
    PhysicalGoal, PhysicalNeedsView, PhysicalPerception, PolicyDiagnostic, PolicyDiagnosticKind,
    PolicyFailureReason, PolicyReason, SIGNAL_TICKS, SignalEvent, WorldPosition,
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
/// A body lies where someone died this long (three simulated hours).
const BODY_TICKS: u64 = 3 * 60 * 60 * 60;
/// Ticks it takes to make something by hand (knapping a blade).
const CRAFT_TICKS: u64 = 300;
/// Anger at someone whose tip proved wrong (before temper), and how much a
/// right one soothes.
const MISLED_ANGER: u8 = 16;
const SOOTHED: u8 = 8;
/// Surprise at a belief proved wrong; relief when a pressing need eases;
/// frustration when a trip has to be given up.
const SURPRISE_WRONG: u8 = 140;
/// Anger at a rival seen beside one's partner (before temper).
const RIVAL_ANGER: u8 = 8;
/// About one sighting in this many of a rival turns jealousy into anger.
const JEALOUS_ODDS: u64 = 16;
/// Anger from which someone close by gets confronted, and how close.
const FURIOUS: u8 = 128;
const CONFRONT_REACH: u64 = 3;
/// Seconds after a stand-off before it will start another.
const CONFRONT_COOLDOWN: u32 = 3_600;
/// Shame at being set right in front of others.
const CORRECTED_SHAME: u8 = 100;
const RELIEF_EASED: u8 = 140;
const FRUSTRATED_STALL: u8 = 60;
/// Faces farther than this (cells) are read less surely.
const FACE_READING_DISTANCE: u64 = 3;
/// Ability with words (0-255) from which each hearing counts twice.
const QUICK_EAR: u8 = 176;
/// One in this many words a child hears is picked up with a vowel changed.
const SOUND_SHIFT_ODDS: u64 = 40;
/// How long someone mourns a person close to them (30 simulated minutes).
const GRIEF_SECONDS: u32 = 30 * 60;
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
        // Found it: tell whoever pointed it out, showing what was there.
        if let Some(speaker) = context.teller {
            dialogue.plan_report(
                PendingCorrection {
                    speaker,
                    form,
                    misread: believed,
                    actual: believed,
                    place: check.place,
                    since: context.now,
                },
                context.now,
            );
        }
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
    // Giving up after searching teaches nothing about the word: pointing is
    // rough, and the thing may just be elsewhere. Only a look at the spot can.
    if !check.up_close {
        return;
    }
    // Only an alternative the listener had actually weighed counts: a stale tip
    // (the berries were eaten) teaches nothing about the word.
    let Some(actual) = check.alternative.filter(|alternative| {
        crate::cognition::concept_topic(*alternative).is_some_and(|topic| match topic {
            GestureTopic::Place(kind) => context.visible[kind.index()],
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
        let season = self.season();
        self.breathe_near_bodies(agent, origin);
        self.pair_up(agent, perception);
        let now = belief_seconds(self.time);
        let heading = self
            .population
            .exploration_heading(agent)
            .unwrap_or(ExplorationHeading::North);
        let personality = self.personality_in_use(agent);
        let crafting = self.ability(agent, crate::Activity::Crafting);
        let night = self.is_night();
        let wayfinding = self
            .innate_personality(agent)
            .talent(crate::Activity::Wayfinding);
        let at = self.time;
        let latest_signal = self.next_signal_id;
        let beg = self.beg_target(agent, perception);
        let teacher = self.ask_target(agent, perception);
        let food = self.food_values(agent);
        self.check_leads(agent, perception, now);
        // Far ahead, the way it's been walking (or wherever its eyes wander
        // when standing still): landmarks, vaguely.
        let came_from = self
            .minds
            .get(agent)
            .and_then(|mind| mind.map.came_from(origin));
        let glance = (self.time.ticks() / 600 + u64::from(agent.get())) % 8;
        let facing = came_from.map_or_else(
            || crate::policy::ExplorationHeading::North.rotated(glance as i8),
            |from| crate::policy::heading_toward(from, origin),
        );
        // Standing still, it looks again only once its eyes have turned.
        let turned = self
            .minds
            .get(agent)
            .is_none_or(|mind| mind.glanced != glance as u8 + 1);
        let sightings = if came_from.is_some() || turned {
            self.minds.get_mut(agent).glanced = if came_from.is_some() {
                0
            } else {
                glance as u8 + 1
            };
            self.far_sight(agent, origin, facing)
        } else {
            Vec::new()
        };
        let (seen_danger, seen_prey, wary) = self.animals_of_interest(agent, origin, perception);
        // Young children don't hunt.
        let pregnant = self
            .motherhood(agent)
            .is_some_and(|motherhood| motherhood.pregnant);
        let hunts = self.age_of(agent) >= crate::HUNTING_AGE && !pregnant;
        // Fit for heavy work: grown, not pregnant, not worn out or starving.
        let able = self.fit_for_heavy_work(agent)
            && !needs.rest.threshold_reached
            && !needs.hunger.threshold_reached;
        let (alarm, quarry) = self.minds.get_mut(agent).dialogue.current_leads(now);
        let near = |place: WorldPosition, within: u64| {
            origin.x.abs_diff(place.x).max(origin.y.abs_diff(place.y)) <= within
        };
        // A warned-about spot as close as a seen wolf would be is a danger
        // even out of sight (any farther, and a hut near the spot has them
        // running off and coming back all night). A tip about
        // something to hunt is worth following when nothing better is in view.
        let alarm_place = alarm
            .map(|lead| lead.place())
            .filter(|place| near(*place, FLEE_DISTANCE));
        let danger = seen_danger.or(alarm_place);
        let quarry_place = quarry
            .map(|lead| lead.place())
            .filter(|_| seen_prey.is_none());
        let (warn, recruit) = self.signals_worth_making(agent, perception, now);
        let visible = visible_kinds(origin, perception);
        let spent = spent_kinds(perception);
        let mut greetings: Vec<AgentId> = Vec::new();
        let mut grumble = None;
        let founder_names: Vec<(AgentId, crate::Name)> = if self.minds.is_founder(agent) {
            let family = agent.get() / crate::FAMILY_SIZE;
            perception
                .agents
                .iter()
                .filter(|other| {
                    other.id.get() / crate::FAMILY_SIZE == family
                        && self.minds.is_founder(other.id)
                        && !self.minds.is_newcomer(other.id)
                        && !self.minds.is_newcomer(agent)
                })
                .map(|other| (other.id, self.life_of(other.id).name))
                .collect()
        } else {
            Vec::new()
        };
        // A body in view it would bury: anyone's, once it has learned the dead
        // are better buried; otherwise only family's and close friends'.
        let body_to_bury = self.minds.get(agent).and_then(|mind| {
            let knows = mind.crafts.knows_burial();
            self.unburied_in(perception.area)
                .into_iter()
                .filter(|(dead, _)| {
                    knows
                        || mind.social.slot_of(*dead).is_some_and(|slot| {
                            mind.social.tie(slot).is_some()
                                || mind.social.familiarity(slot) >= crate::FRIEND_FAMILIARITY
                        })
                })
                .map(|(_, position)| position)
                .min_by_key(|position| {
                    (
                        origin.x.abs_diff(position.x) + origin.y.abs_diff(position.y),
                        position.y,
                        position.x,
                    )
                })
        });
        // Bodies lie where people died for a while; anyone passing sees them.
        let bodies: Vec<AgentId> = self
            .death_records
            .iter()
            .filter(|record| {
                self.time.ticks().saturating_sub(record.at.ticks()) <= BODY_TICKS
                    && perception.area.contains(record.position)
            })
            .map(|record| record.agent)
            .collect();
        let bodies_seen = !bodies.is_empty();
        // Its partner close beside another adult of the partner's opposite sex
        // who isn't family: a rival.
        let rival = self
            .minds
            .get(agent)
            .and_then(|mind| mind.social.partner())
            .and_then(|(_, partner)| {
                let at = perception
                    .agents
                    .iter()
                    .find(|seen| seen.id == partner)?
                    .position;
                let partner_sex = self.life_of(partner).sex;
                perception
                    .agents
                    .iter()
                    .find(|seen| {
                        seen.id != agent
                            && seen.id != partner
                            && seen.activity != AgentActivity::Dead
                            && seen
                                .position
                                .x
                                .abs_diff(at.x)
                                .max(seen.position.y.abs_diff(at.y))
                                <= 1
                            && self.life_of(seen.id).sex != partner_sex
                            && self.age_of(seen.id) >= crate::ADULT_AGE
                            && self
                                .minds
                                .get(partner)
                                .is_none_or(|mind| mind.social.tie_with(seen.id).is_none())
                    })
                    .map(|seen| seen.id)
            });
        // Those around who look in need (moving slowly, hungry or thirsty
        // looking): worth showing a place they may not know.
        let needy: Vec<AgentId> = perception
            .agents
            .iter()
            .filter(|other| other.id != agent && self.visible_urgency(other.id) >= LOOKS_IN_NEED)
            .map(|other| other.id)
            .collect();
        let hint_outcomes = &mut self.hint_outcomes;
        let farm_events = &mut self.farm_events;
        let lessons = &mut self.lesson_events;
        let griefs = &mut self.grief_events;
        let mind = self.minds.get_mut(agent);
        for other in &perception.agents {
            if other.id != agent && other.activity != AgentActivity::Dead {
                // Meeting someone again after a while: call out their name.
                let reunion = mind.social.slot_of(other.id).is_some_and(|slot| {
                    mind.social.name(slot).is_some()
                        && now.saturating_sub(mind.social.last_seen(slot))
                            >= super::names::GREETING_GAP_SECONDS
                });
                if reunion && can_watch(other.activity) {
                    greetings.push(other.id);
                }
                if let Some(slot) = mind.notice(other.id, other.position, now) {
                    // Asleep beside a hut: that's where they live.
                    if other.activity == AgentActivity::Sleeping
                        && let Some(hut) = hut_beside(perception, other.position)
                    {
                        mind.social.saw_asleep_at(slot, hut);
                    }
                    // Seen right at a place: they know it's there.
                    for kind in places_beside(perception, other.position, now) {
                        mind.social.credit_knowing(slot, kind);
                    }
                    // Founders know the names of their own family's founders.
                    if let Some(name) = founder_names
                        .iter()
                        .find(|(id, _)| *id == other.id)
                        .map(|(_, name)| *name)
                    {
                        mind.social.learn_name(slot, name);
                    }
                }
            }
        }
        // The body of someone close: mourn them, then let them go.
        for body in bodies {
            let close = mind.social.slot_of(body).is_some_and(|slot| {
                mind.social.tie(slot).is_some()
                    || mind.social.familiarity(slot) >= crate::FRIEND_FAMILIARITY
            });
            if close {
                mind.grief_until = now.saturating_add(GRIEF_SECONDS);
                mind.social.forget(body);
                griefs.push(crate::GriefEvent {
                    agent,
                    lost: body,
                    at,
                });
            }
        }
        mind.social.update_whereabouts(perception.area, |id| {
            perception.agents.iter().any(|other| other.id.get() == id)
        });

        // Angry with someone, with a friend about: it grumbles about them.
        if now.saturating_sub(mind.grumbled_at) >= super::names::GRUMBLE_GAP_SECONDS
            && let Some((about, name)) = mind.social.grudge()
            && perception.agents.iter().any(|other| {
                other.id != agent
                    && other.id != about
                    && can_watch(other.activity)
                    && mind.social.slot_of(other.id).is_some_and(|slot| {
                        mind.social.familiarity(slot) >= crate::FRIEND_FAMILIARITY
                            && mind.social.trust(slot) >= crate::cognition::DEFAULT_TRUST
                            && mind.social.anger(slot) < crate::cognition::ANGRY
                    })
            })
        {
            mind.grumbled_at = now;
            grumble = Some((about, name));
        }
        // Its home in view with no hut standing there: it's gone.
        if let Some(hut) = mind.home.hut()
            && perception.area.contains(hut)
            && !perception.structures.iter().any(|structure| {
                structure.position == hut
                    && structure.kind == crate::StructureKind::Shelter
                    && structure.state == crate::StructureState::Complete
            })
        {
            mind.home.lost();
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
            grief_until,
            last_eligible_seen: _,
            stored_food,
            stall: _,
            character,
            feelings,
            confronted_at,
            asking: _,
            habits,
            home,
            model,
            intention,
            avoid,
            seasons,
            sown,
            sick_until: _,
            grumbled_at: _,
            glanced: _,
        } = mind;
        // A bush bearing berries where it once ate some on open ground: seeds grow.
        let fruit = perception
            .resources
            .iter()
            .filter(|found| {
                matches!(
                    found.resource.kind,
                    crate::Material::Berries | crate::Material::Bitterberries
                )
            })
            .map(|found| found.position);
        if sown.sprouted(fruit, now) {
            crafts.saw_planting();
            farm_events.push(crate::FarmEvent {
                agent,
                at: origin,
                planted: false,
                watchers: 0,
            });
        }
        // How hard this season is, and what to get ready if the next one was
        // hard last time it came round: being where a hut is if it was cold,
        // having food on hand if it was hungry (in the last month before it).
        let level =
            |need: crate::NeedLevelView| crate::cognition::need_level(need.value, need.threshold);
        seasons.felt(season as usize, level(needs.exposure), level(needs.hunger));
        // Only in the last month of the season before: the rest of it is for
        // the season's own work.
        let late = u64::from(now) % crate::season::SEASON_SECONDS
            >= crate::season::SEASON_SECONDS - crate::MONTH_SECONDS as u64;
        let ready = {
            let [cold, hungry] = seasons.remembered((season as usize + 1) % 4);
            if !late {
                None
            } else if cold >= 100 {
                Some(crate::cognition::Facts::SHELTER)
            } else if hungry >= 100 {
                Some(crate::cognition::Facts::HAS_FOOD)
            } else {
                None
            }
        };
        model.know_how(
            crafts.knows_hearths(),
            crafts.knows_huts(),
            crafts.knows_knapping(),
        );
        let facts = crate::policy::perceived_facts(perception, inventory, needs, food, now);
        let intended = std::cell::Cell::new(*intention);
        let lacked = std::cell::Cell::new(None);
        let making = std::cell::Cell::new(None);
        let asking = std::cell::Cell::new(None);
        let own_home = home.hut();
        // Without a home of its own, its family's is home; huts it has seen
        // others live in are theirs.
        let home = own_home.or_else(|| people.family_hut());
        let others_huts: Vec<WorldPosition> = people
            .others_huts()
            .filter(|hut| Some(*hut) != home)
            .collect();
        // Late in autumn, remembering a hungry winter: fill the hut with food.
        // Worried about it, all autumn long.
        let stocking = season == crate::Season::Autumn
            && (late || feelings.strong(crate::Feeling::Worry))
            && seasons.remembered(crate::Season::Winter as usize)[1] >= 100;
        let stored_food = stored_food.map(crate::agent::CompactPosition::world);
        let grief_until = *grief_until;
        let explored = map.explored_tile_count();
        let temper = u32::from(personality.temper);
        let tempered = |amount: u8| (u32::from(amount) * (64 + temper) / 192).min(255) as u8;
        map.observe(
            agent.get(),
            origin,
            perception,
            now,
            &mut |check: HintCheck| {
                let teller = check.teller.and_then(|slot| people.agent_in(slot));
                // Stripped bushes in view explain an empty spot: the tip was right
                // but stale, which says nothing about the teller or the word.
                if !check.confirmed && spent[check.kind.index()] {
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
                if let Some(slot) = check.teller {
                    people.hint_checked(slot, check.confirmed);
                    // Misled, it's angry with whoever said so (the hot-tempered
                    // more); proved right, it lets some of that go.
                    if check.confirmed {
                        people.soothe(slot, SOOTHED);
                    } else {
                        people.provoke(slot, tempered(MISLED_ANGER));
                    }
                }
                if !check.confirmed {
                    feelings.feel(crate::Feeling::Surprise, SURPRISE_WRONG);
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
        for sighting in &sightings {
            map.remember_seen_far(
                sighting.kind,
                sighting.estimate,
                sighting.uncertainty,
                now,
                facing,
                super::sight::Sighting::CONFIDENCE,
            );
        }
        // New ground covered: practice at finding the way.
        let explored_new = map.explored_tile_count() > explored;
        if explored_new {
            character.practise(crate::Activity::Wayfinding, wayfinding);
        }
        // How it feels, from what it's going through (docs/plans/FEELINGS.md).
        let misread_faces = {
            use crate::Feeling;
            feelings.fade(now, personality.steadiness);
            // The timid feel fear more, the bold less.
            let timid = |amount: u32| {
                (amount * (320 - u32::from(personality.boldness)) / 192).min(255) as u8
            };
            let alone = !perception
                .agents
                .iter()
                .any(|other| other.id != agent && other.activity != AgentActivity::Dead);
            if danger.is_some() {
                feelings.feel(Feeling::Fear, timid(40));
            }
            if bodies_seen {
                feelings.feel(Feeling::Fear, timid(20));
            }
            if night && alone {
                feelings.feel(Feeling::Fear, timid(6));
            }
            let pressing = needs.hunger.threshold_reached
                || needs.thirst.threshold_reached
                || needs.exposure.threshold_reached;
            if pressing {
                feelings.feel(Feeling::Misery, 8);
                feelings.ease(Feeling::Contentment, 8);
            } else {
                feelings.ease(Feeling::Misery, 4);
                if !alone {
                    feelings.feel(Feeling::Contentment, 6);
                }
            }
            if alone {
                let lonely = (3 * (64 + u32::from(personality.sociability)) / 192) as u8;
                feelings.feel(Feeling::Loneliness, lonely);
            } else {
                feelings.ease(Feeling::Loneliness, 12);
            }
            if explored_new {
                feelings.ease(Feeling::Boredom, 40);
            } else {
                feelings.feel(Feeling::Boredom, 2);
            }
            // A need that pressed has eased: relief.
            if !pressing && feelings.level(Feeling::Misery) >= crate::cognition::SHOWS {
                feelings.feel(Feeling::Relief, RELIEF_EASED);
            }
            // Autumn, and a winter remembered as hungry: worry (the steady less).
            if season == crate::Season::Autumn
                && seasons.remembered(crate::Season::Winter as usize)[1] >= 100
            {
                let worry = (6 * (320 - u32::from(personality.steadiness)) / 192) as u8;
                feelings.feel(Feeling::Worry, worry);
            }
            // A plan under way gives hope.
            if intention.aim().is_some() {
                feelings.feel(Feeling::Hope, 3);
            }
            // The dead disgust.
            if bodies_seen {
                feelings.feel(Feeling::Disgust, 30);
            }
            // Jealous of a rival close to its partner, and angry with them.
            if let Some(rival) = rival {
                feelings.feel(Feeling::Jealousy, 30);
                // Now and then it boils over into anger at the rival.
                let roll =
                    mix(u64::from(agent.get()) << 32 ^ u64::from(rival.get()) ^ u64::from(now));
                if roll % JEALOUS_ODDS == 0
                    && let Some(slot) = people.slot_of(rival)
                {
                    people.provoke(slot, tempered(RIVAL_ANGER));
                }
            }
            // Hungry with nothing to eat, looking at food in another family's
            // hut: envy.
            if needs.hunger.threshold_reached
                && perception.structures.iter().any(|structure| {
                    structure.kind == crate::StructureKind::Shelter
                        && others_huts.contains(&structure.position)
                        && structure
                            .stored
                            .carried()
                            .any(|(material, _)| food.is_food(material))
                })
            {
                feelings.feel(Feeling::Envy, 12);
            }
            // Someone it's angry with in view: it tenses up.
            if perception.agents.iter().any(|other| {
                people
                    .slot_of(other.id)
                    .is_some_and(|slot| people.anger(slot) >= crate::cognition::ANGRY)
            }) {
                feelings.feel(Feeling::Frustration, 10);
            }
            // Faces around: fear spreads to those who see it. Strangers'
            // faces, and faces far off, are sometimes read as their lookalike.
            let mut misread = 0;
            for &(other, shown) in &perception.expressions {
                if other == agent {
                    continue;
                }
                let familiar = people
                    .slot_of(other)
                    .is_some_and(|slot| people.familiarity(slot) >= crate::FRIEND_FAMILIARITY);
                let far = perception
                    .agents
                    .iter()
                    .find(|seen| seen.id == other)
                    .is_some_and(|seen| {
                        seen.position
                            .x
                            .abs_diff(origin.x)
                            .max(seen.position.y.abs_diff(origin.y))
                            > FACE_READING_DISTANCE
                    });
                let roll =
                    mix(u64::from(agent.get()) << 32 ^ u64::from(other.get()) ^ u64::from(now));
                let read = if (!familiar || far) && roll % 3 == 0 {
                    misread += 1;
                    shown.lookalike()
                } else {
                    shown
                };
                // Seeing fear unsettles, but alone it never frightens: it
                // stops short of what shows.
                if read == Feeling::Fear && feelings.level(Feeling::Fear) < crate::cognition::SHOWS
                {
                    feelings.feel(Feeling::Fear, timid(4));
                }
            }
            misread
        };
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
        let cooldown = Personality::scale(personality.sociability, 110, 10) as u32
            / if feelings.strong(crate::Feeling::Contentment) {
                2
            } else {
                1
            };
        // Setting the record straight with a speaker in view comes before sharing.
        let correction = dialogue.correction(now).filter(|correction| {
            perception
                .agents
                .iter()
                .any(|other| other.id == correction.speaker && can_watch(other.activity))
        });
        // A correction for someone out of sight: go to where they were last seen.
        let seek = (correction.is_none())
            .then(|| dialogue.correction(now))
            .flatten()
            .and_then(|pending| people.whereabouts(pending.speaker));
        // The kinds of place someone around who looks in need isn't believed
        // to know (strangers are believed to know nothing).
        let unknown = perception
            .agents
            .iter()
            .filter(|other| needy.contains(&other.id) && can_watch(other.activity))
            .fold(0_u8, |unknown, other| {
                unknown
                    | !people
                        .slot_of(other.id)
                        .map_or(0, |slot| people.knows(slot))
            });
        let share = correction
            .map(|correction| (correction.place, u8::MAX))
            .or_else(|| {
                (company && map.share_ready(now, cooldown))
                    .then(|| map.shareable(perception.area, unknown))
                    .flatten()
            });
        // Visit friends only when alone, or a partner who is out of sight;
        // sociable people remember them for longer.
        let partner_away = people
            .partner()
            .is_some_and(|(_, partner)| !perception.agents.iter().any(|other| other.id == partner));
        let friend_target = (!company || partner_away)
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
        // How it is now: needs past threshold, season, shelter or fire in view.
        let situation = {
            let shelter = perception.structures.iter().any(|structure| {
                structure.kind == crate::StructureKind::Shelter
                    && structure.state == crate::StructureState::Complete
            });
            let fire = perception
                .structures
                .iter()
                .any(|structure| structure.kind.burns() && structure.working(now));
            crate::cognition::Situation::new(
                [
                    needs.hunger.threshold_reached,
                    needs.thirst.threshold_reached,
                    needs.rest.threshold_reached,
                    needs.exposure.threshold_reached,
                ],
                season,
                shelter,
                fire,
            )
        };
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
                crafts: *crafts,
                feelings: *feelings,
                crafting,
                teacher: teacher.map(|(_, position)| position),
                knows_planting: crafts.knows_planting(),
                knows_burial: crafts.knows_burial(),
                confront: perception
                    .agents
                    .iter()
                    .filter(|_| now.saturating_sub(*confronted_at) >= CONFRONT_COOLDOWN)
                    .filter(|seen| {
                        seen.id != agent
                            && can_watch(seen.activity)
                            && seen
                                .position
                                .x
                                .abs_diff(origin.x)
                                .max(seen.position.y.abs_diff(origin.y))
                                <= CONFRONT_REACH
                            && people
                                .slot_of(seen.id)
                                .is_some_and(|slot| people.anger(slot) >= FURIOUS)
                    })
                    .min_by_key(|seen| (seen.position.y, seen.position.x))
                    .map(|seen| seen.position),
                knows_knapping: crafts.knows_knapping(),
                came_from: map.came_from(origin),
                grieving: now < grief_until,
                knows_huts: crafts.knows_huts(),
                able,
                stored_food,
                home,
                others_huts: &others_huts,
                stocking,
                // Sowing is a spring job: what's sown bears from summer.
                plants: crafts.knows_planting() && season == crate::Season::Spring,
                body: body_to_bury.filter(|_| able),
                hunts,
                avoid: avoid.place(now),
                ready,
                habits: Some((&*habits, situation)),
                model: &*model,
                plans: crate::cognition::PLANS,
                facts,
                intention: &intended,
                lacked: &lacked,
                making: &making,
                asking: &asking,
            },
        );
        {
            let mind = self.minds.get_mut(agent);
            let before = mind.intention;
            let after = intended.get();
            mind.intention = after;
            if let Some((choice, fact)) = lacked.get() {
                mind.model.lacked(choice, fact);
            }
            // Asking someone to show it how to make a tool.
            if let (Some(tool), Some((teacher, at))) = (asking.get(), teacher) {
                mind.asking = Some(tool);
                mind.dialogue.plan_request(teacher, at);
            }
            // A project's making step or a tinkering: do that very thing.
            if let Some(attempt) = making.get() {
                mind.crafts.set_pending(attempt);
            }
            self.note_intention(agent, before, after, facts);
        }
        self.minds.get_mut(agent).map.mark_decision(origin);
        self.faces_misread += misread_faces;
        // Getting no closer to where it's headed, decision after decision: it
        // can't get there from here, so it stops trying and keeps clear of
        // that place for a while.
        if let Some(destination) = deliberation.toward {
            let distance = origin.x.abs_diff(destination.x) + origin.y.abs_diff(destination.y);
            let mind = self.minds.get_mut(agent);
            if mind.stall.note(destination, distance) {
                mind.map.give_up(destination);
                mind.avoid = crate::cognition::Avoid::of(destination, now);
                if mind.stored_food.map(crate::agent::CompactPosition::world) == Some(destination) {
                    mind.stored_food = None;
                }
                if mind.home.hut() == Some(destination) {
                    mind.home.lost();
                }
                mind.feelings
                    .feel(crate::Feeling::Frustration, FRUSTRATED_STALL);
                self.gave_up += 1;
            }
        }
        // Learn from the last choice by how discomfort changed since, then
        // start judging this one.
        {
            let discomfort = crate::cognition::discomfort(
                [needs.hunger, needs.thirst, needs.rest, needs.exposure]
                    .map(|level| (level.value, level.threshold)),
            );
            let goal = deliberation.selection.goal;
            let reason = deliberation.selection.reason;
            let choice = crate::cognition::Choice::of(goal, reason);
            let action = crate::cognition::Choice::acted(goal, reason, deliberation.trip);
            let levels = [needs.hunger, needs.thirst, needs.rest, needs.exposure]
                .map(|level| crate::cognition::need_level(level.value, level.threshold));
            let mind = self.minds.get_mut(agent);
            let vivid = mind.feelings.strong(crate::Feeling::Surprise)
                || mind.feelings.strong(crate::Feeling::Relief);
            mind.habits.decided(situation, choice, discomfort, vivid);
            mind.model.decided(action, facts, levels, now);
        }
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
        for other in greetings {
            self.call_out(agent, other);
        }
        if let Some((about, name)) = grumble {
            self.grumble(agent, about, name);
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
        // Skilled hands are quicker at what they've practised.
        let skilled = match goal {
            PhysicalGoal::Craft => Some(crate::Activity::Crafting),
            PhysicalGoal::Cook => Some(crate::Activity::Cooking),
            PhysicalGoal::Plant => Some(crate::Activity::Farming),
            _ => None,
        };
        if let Some(activity) = skilled {
            self.practise(agent, activity);
        }
        // Smoking or trying something: settle now what it is.
        let attempt = self.choose_attempt(agent, goal, target, reason);
        let slowness = attempt.map_or(1, |attempt| attempt.method().slowness());
        if let Some(attempt) = attempt {
            self.minds.get_mut(agent).crafts.set_pending(attempt);
        }
        let duration = match (goal, reason) {
            (PhysicalGoal::Hunt, _) => super::HUNT_TICKS,
            (PhysicalGoal::WarmUp | PhysicalGoal::TendFire, _) => WARM_UP_TICKS,
            (PhysicalGoal::Craft | PhysicalGoal::Plant | PhysicalGoal::Cook, _) => {
                let activity = skilled.expect("matched above");
                let dug = if goal == PhysicalGoal::Plant && self.digs(agent) {
                    2
                } else {
                    1
                };
                crate::cognition::by_ability(CRAFT_TICKS, self.ability(agent, activity)) * slowness
                    / dug
            }
            (PhysicalGoal::Bury, _) => CRAFT_TICKS / if self.digs(agent) { 2 } else { 1 },
            (PhysicalGoal::Repair, _) => crate::cognition::by_ability(
                CRAFT_TICKS,
                self.ability(agent, crate::Activity::Building),
            ),
            (PhysicalGoal::WallInStone, _) => crate::cognition::by_ability(
                crate::SHELTER_BUILD_TICKS / 2,
                self.ability(agent, crate::Activity::Building),
            ),
            (PhysicalGoal::Store | PhysicalGoal::Fetch | PhysicalGoal::Drop, _) => WARM_UP_TICKS,
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

    /// Whether `agent` carries something to dig with; using it wears it.
    fn digs(&mut self, agent: AgentId) -> bool {
        let tool = self.population.inventory(agent).and_then(|inventory| {
            inventory
                .carried()
                .map(|(material, _)| material)
                .find(|material| material.properties().digs)
        });
        if let Some(tool) = tool {
            self.wear(agent, tool);
        }
        tool.is_some()
    }

    /// What a smoking or an experiment will be: the known smoking recipe for
    /// what it carries, or an untried combination of what it carries by the
    /// methods at hand (at a fire for cooking work, by hand or edge otherwise).
    fn choose_attempt(
        &self,
        agent: AgentId,
        goal: PhysicalGoal,
        target: WorldPosition,
        reason: PolicyReason,
    ) -> Option<crate::cognition::Attempt> {
        let inventory = self.population.inventory(agent)?;
        let crafts = self.minds.get(agent)?.crafts;
        // Cooking work at a rack dries; at a fire it roasts or smokes.
        let at_rack = goal == PhysicalGoal::Cook
            && self.drying_rack(
                self.structures
                    .structure_at(target)
                    .and_then(|id| self.structures.view(id)),
            );
        let keeping = if at_rack {
            crate::Method::Rack
        } else {
            crate::Method::Smoke
        };
        match reason {
            PolicyReason::Preserving => crate::RECIPES
                .iter()
                .enumerate()
                .filter(|(index, recipe)| {
                    recipe.method == keeping
                        && crafts.knows(*index)
                        && recipe
                            .inputs
                            .iter()
                            .all(|&(input, count)| inventory.amount(input) >= count)
                })
                .map(|(_, recipe)| crate::cognition::Attempt::new(recipe.kinds(), recipe.method))
                .next(),
            PolicyReason::Experimenting => {
                let carried: Vec<crate::Material> =
                    inventory.carried().map(|(material, _)| material).collect();
                let edge = inventory
                    .carried()
                    .any(|(material, _)| material.properties().cutting);
                let methods: &[crate::Method] = match goal {
                    PhysicalGoal::Cook if at_rack => &[crate::Method::Rack],
                    PhysicalGoal::Cook => &[crate::Method::Fire, crate::Method::Smoke],
                    _ if edge => &[crate::Method::Hand, crate::Method::Edge],
                    _ => &[crate::Method::Hand],
                };
                let roll = mix(self.config.life_seed
                    ^ 0x5452_5920
                    ^ u64::from(agent.get()) << 32
                    ^ self.time.ticks());
                crafts.untried(&carried, methods, roll)
            }
            _ => None,
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
            if let Some(tool) = self.minds.get_mut(sender).asking.take() {
                return self.apply_show_request(sender, giver, tool);
            }
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
            .and_then(|mind| mind.lexicon.produce(topic.concept()))
            .or_else(|| self.coin_word(sender, topic.concept()));
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
            let shifts = self.age_of(watcher.id) < crate::ADULT_AGE
                && mix(self.config.life_seed
                    ^ 0x5348_4946
                    ^ u64::from(watcher.id.get()) << 32
                    ^ id)
                    % SOUND_SHIFT_ODDS
                    == 0;
            // Someone with an ear for words takes in each hearing twice over.
            let quick_ear = self.ability(watcher.id, crate::Activity::Words) >= QUICK_EAR;
            let words_talent = self
                .innate_personality(watcher.id)
                .talent(crate::Activity::Words);
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
            // Whoever points a place out (as it reads the gesture) knows it.
            if let GestureTopic::Place(kind) = understanding.topic
                && let Some(slot) = mind.social.slot_of(public.sender)
            {
                mind.social.credit_knowing(slot, kind);
            }
            // Words are learned from the listener's own reading, right or wrong.
            if let Some(form) = public.vocal {
                let heard_as = understanding.topic.concept();
                // A child now and then picks a word up with a vowel changed.
                let learned = if shifts { form.shifted() } else { form };
                mind.lexicon.hear_with_evidence(learned, heard_as);
                if quick_ear {
                    mind.lexicon.hear_with_evidence(learned, heard_as);
                }
                mind.character
                    .practise(crate::Activity::Words, words_talent);
                if learned != form {
                    self.word_events.push(crate::WordEvent::Shifted {
                        agent: watcher.id,
                        heard: form,
                        learned,
                    });
                }
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
            let teller = match understanding.topic {
                GestureTopic::Place(_) => mind.notice(public.sender, public.origin, now),
                _ => None,
            };
            // Someone it's angry with is believed less.
            let trust = teller.map_or(crate::DEFAULT_TRUST, |slot| {
                mind.social
                    .trust(slot)
                    .saturating_sub(mind.social.anger(slot) / 2)
            });
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
                        far: false,
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
                    || (kind == LandmarkKind::BERRIES && hunger >= URGENT_NEED))
            {
                let source = HintSource {
                    teller,
                    form: public.vocal,
                    alternative: Some(best),
                    signal: id,
                    bearing: Some(bearing),
                    far: false,
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
                    // Set right in front of others: ashamed.
                    if perception.agents.len() > 2 {
                        mind.feelings.feel(crate::Feeling::Shame, CORRECTED_SHAME);
                    }
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
        if !self.someone_within_call(agent) {
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
                        far: false,
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
        let report = correction.is_report();
        let intent = UtteranceIntent {
            effect: if report {
                DesiredEffect::Inform
            } else {
                DesiredEffect::Correct
            },
            topic: actual,
            place: correction.place,
        };
        let urgency = self.visible_urgency(sender);
        let mime = self.mime_of(sender, actual);
        let mut public = express(sender, from, intent, mime, Some(correction.form), urgency)
            .ok_or(PolicyFailureReason::TargetUnavailable)?;
        if !report {
            public.negated = Some(self.mime_of(sender, misread));
        }
        // Holds up the thing itself if it has some on it.
        public.shown = match correction.actual {
            Concept::Material(material) => self
                .population
                .inventory(sender)
                .filter(|inventory| inventory.amount(material) > 0)
                .map(|_| material),
            _ => None,
        };
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

    /// With no word for `concept`, `agent` sometimes makes one up (curious
    /// people more often) and says it.
    fn coin_word(&mut self, agent: AgentId, concept: Concept) -> Option<VocalForm> {
        let curiosity = self.personality_in_use(agent).curiosity;
        let roll = mix(self.config.life_seed
            ^ 0x434f_494e
            ^ u64::from(agent.get()) << 32
            ^ self.time.ticks());
        if roll % 256 >= Personality::scale(curiosity, 16, 112) as u64 {
            return None;
        }
        let form = self.minds.get_mut(agent).lexicon.coin(concept, roll >> 8)?;
        self.word_events.push(crate::WordEvent::Coined {
            agent,
            form,
            concept,
        });
        Some(form)
    }

    /// Having made (or skinned) a thing with others watching, `maker` says
    /// what it is: its word, or a new one it makes up (most often from its
    /// word for what the thing is made from, with an ending). Watchers see the
    /// thing and hear the word, so they learn it (a child now and then with a
    /// vowel changed).
    pub(super) fn name_made(&mut self, maker: AgentId, concept: Concept, watchers: &[AgentId]) {
        if watchers.is_empty() {
            return;
        }
        let roll = mix(self.config.life_seed
            ^ 0x4e41_4d45
            ^ u64::from(maker.get()) << 32
            ^ self.time.ticks());
        let lexicon = &mut self.minds.get_mut(maker).lexicon;
        let form = match lexicon.produce(concept) {
            Some(form) => form,
            None => {
                let base = concept
                    .root()
                    .and_then(|root| lexicon.produce(root))
                    .filter(|base| base.parts().is_none());
                let coined = base
                    .and_then(|base| lexicon.coin_compound(concept, base, roll >> 8))
                    .or_else(|| lexicon.coin(concept, roll >> 16));
                let Some(form) = coined else {
                    return;
                };
                self.word_events.push(crate::WordEvent::Coined {
                    agent: maker,
                    form,
                    concept,
                });
                form
            }
        };
        for &watcher in watchers {
            let shifts = self.age_of(watcher) < crate::ADULT_AGE
                && mix(roll ^ u64::from(watcher.get()) << 40) % SOUND_SHIFT_ODDS == 0;
            let learned = if shifts { form.shifted() } else { form };
            self.minds
                .get_mut(watcher)
                .lexicon
                .hear_with_evidence(learned, concept);
            if learned != form {
                self.word_events.push(crate::WordEvent::Shifted {
                    agent: watcher,
                    heard: form,
                    learned,
                });
            }
        }
    }

    /// New words and sound shifts from the latest tick (for logs and tools).
    pub fn word_events(&self) -> &[crate::WordEvent] {
        &self.word_events
    }

    /// People who began mourning during the latest tick (for logs and tools).
    /// Discoveries, stone walls, stand-offs and takings from the latest tick.
    pub fn life_events(&self) -> &[crate::LifeEvent] {
        &self.life_events
    }

    /// The feeling showing on `agent`'s face, if any (what others can see).
    pub fn face(&self, agent: AgentId) -> Option<crate::Feeling> {
        self.minds.get(agent)?.feelings.shown()
    }

    pub fn grief_events(&self) -> &[crate::GriefEvent] {
        &self.grief_events
    }

    /// Plans formed, completed, and given up this tick.
    pub fn plan_events(&self) -> &[crate::cognition::PlanEvent] {
        &self.plan_events
    }

    /// What someone is set on doing, if anything.
    pub fn intention(&self, agent: AgentId) -> Option<crate::cognition::IntentionView> {
        self.minds.get(agent)?.intention.view()
    }

    /// Logs what became of `agent`'s intention between `before` and `after`.
    fn note_intention(
        &mut self,
        agent: AgentId,
        before: crate::cognition::Intention,
        after: crate::cognition::Intention,
        facts: crate::cognition::Facts,
    ) {
        use crate::cognition::{Facts, PlanEvent, PlanEventKind};
        let at = self.time;
        let mut log = |kind, intention: crate::cognition::Intention| {
            if let Some(intention) = intention.view() {
                self.plan_events.push(PlanEvent {
                    agent,
                    at,
                    kind,
                    intention,
                });
            }
        };
        let replaced =
            before.aim().is_some() && after.aim().is_some() && before.formed != after.formed;
        let ended = before.aim().is_some() && after.aim().is_none();
        if replaced || ended {
            let achieved = match before.aim() {
                Some(crate::cognition::Goal::Need(need)) => !facts.has(Facts::NEEDS[need]),
                Some(crate::cognition::Goal::Ready(fact)) => facts.has(fact),
                None => false,
            };
            let kind = if achieved && !replaced {
                PlanEventKind::Completed
            } else {
                PlanEventKind::Revised
            };
            log(kind, before);
        }
        if after.aim().is_some() && (before.aim().is_none() || before.formed != after.formed) {
            log(PlanEventKind::Formed, after);
        }
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

    /// A copy of what `agent` knows about places and people. `None` before it has a mind.
    /// The hut `agent` has made its home, if any.
    pub fn home_of(&self, agent: AgentId) -> Option<WorldPosition> {
        self.minds.get(agent)?.home.hut()
    }

    pub fn mental_map(&self, agent: AgentId) -> Option<MentalMapView> {
        let mind = self.minds.get(agent)?;
        Some(MentalMapView {
            agent,
            personality: self.personality_in_use(agent),
            abilities: crate::Activity::ALL.map(|activity| self.ability(agent, activity)),
            skills: mind.character.skills(),
            feeling: mind.feelings.shown(),
            can_make: {
                let mut made: Vec<crate::Material> = crate::RECIPES
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| mind.crafts.knows(*index))
                    .map(|(_, recipe)| recipe.output)
                    .collect();
                made.dedup();
                made
            },
            landmarks: mind.map.views().collect(),
            explored_tiles: mind.map.explored_tile_count(),
            child: mind.child,
            affordances: mind.affordances.views().collect(),
            fauna: mind.fauna.views().collect(),
            knows_hearths: mind.crafts.knows_hearths(),
            knows_knapping: mind.crafts.knows_knapping(),
            knows_huts: mind.crafts.knows_huts(),
            habits: mind.habits.views().collect(),
            effects: mind.habits.effect_views().collect(),
            home: mind.home.hut(),
            beliefs: mind.model.views().collect(),
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

    /// Bonds a child to a parent: they know each other as family from the start,
    /// and the child knows the parent's other children as siblings.
    pub fn bond(&mut self, child: AgentId, parent: AgentId) -> bool {
        let (Some(parent_at), Some(child_at)) = (
            self.population.view(parent).map(|view| view.position),
            self.population.view(child).map(|view| view.position),
        ) else {
            return false;
        };
        let now = belief_seconds(self.time);
        let (parent_name, child_name) = (self.life_of(parent).name, self.life_of(child).name);
        let mind = self.minds.get_mut(child);
        mind.parent = Some(parent);
        let Some(slot) = mind.social.bond(parent, parent_at, now) else {
            return false;
        };
        // Family know each other's names.
        mind.social.set_tie(slot, Tie::Parent);
        mind.social.learn_name(slot, parent_name);
        let social = &mut self.minds.get_mut(parent).social;
        if let Some(slot) = social.bond(child, child_at, now) {
            social.set_tie(slot, Tie::Child);
            social.learn_name(slot, child_name);
        }
        let siblings: Vec<(AgentId, WorldPosition)> = self
            .population
            .views(usize::MAX)
            .filter(|view| {
                view.id != child
                    && self
                        .minds
                        .get(view.id)
                        .is_some_and(|other| other.parent == Some(parent))
            })
            .map(|view| (view.id, view.position))
            .collect();
        for (sibling, sibling_at) in siblings {
            for (from, to, at) in [(child, sibling, sibling_at), (sibling, child, child_at)] {
                let name = self.life_of(to).name;
                let social = &mut self.minds.get_mut(from).social;
                if let Some(slot) = social.bond(to, at, now) {
                    social.set_tie(slot, Tie::Sibling);
                    social.learn_name(slot, name);
                }
            }
        }
        true
    }

    /// The agent's innate personality (independent of whether the policy uses it).
    pub fn personality(&self, agent: AgentId) -> Option<Personality> {
        self.population
            .view(agent)
            .map(|_| self.innate_personality(agent))
    }

    /// The personality the policy acts on: innate as life has shaped it, with
    /// `social`; else average.
    pub(super) fn personality_in_use(&self, agent: AgentId) -> Personality {
        let innate = self.innate_personality(agent);
        self.minds
            .get(agent)
            .map_or(innate, |mind| mind.character.shape(innate))
    }

    /// How good `agent` is at `activity` (0-255): talent and practice, or
    /// average for agents without minds.
    pub(super) fn ability(&self, agent: AgentId, activity: crate::Activity) -> u8 {
        let talent = self.innate_personality(agent).talent(activity);
        self.minds
            .get(agent)
            .map_or(talent / 2, |mind| mind.character.ability(activity, talent))
    }

    /// `agent` did `activity` once: its skill grows (only minds hold skill).
    pub(super) fn practise(&mut self, agent: AgentId, activity: crate::Activity) {
        let talent = self.innate_personality(agent).talent(activity);
        self.minds
            .get_mut(agent)
            .character
            .practise(activity, talent);
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

/// The finished hut right beside `position` in `perception`, if any.
fn hut_beside(perception: &PhysicalPerception, position: WorldPosition) -> Option<WorldPosition> {
    perception
        .structures
        .iter()
        .find(|structure| {
            structure.kind == crate::StructureKind::Shelter
                && structure.state == crate::StructureState::Complete
                && structure.position.x.abs_diff(position.x)
                    + structure.position.y.abs_diff(position.y)
                    == 1
        })
        .map(|structure| structure.position)
}

/// Visible urgency (128 = at a need's threshold) from which someone looks in
/// need of being shown a place.
const LOOKS_IN_NEED: u8 = 96;

/// The kinds of place right beside `position` in `perception`: water, a
/// gatherable source, or a finished structure, within 2 cells.
fn places_beside(
    perception: &crate::PhysicalPerception,
    position: WorldPosition,
    now: u32,
) -> impl Iterator<Item = LandmarkKind> + '_ {
    let near =
        move |at: WorldPosition| at.x.abs_diff(position.x).max(at.y.abs_diff(position.y)) <= 2;
    let water = perception
        .drinkable_water
        .iter()
        .any(|water| near(water.position))
        .then_some(LandmarkKind::Water);
    let sources = perception
        .resources
        .iter()
        .filter(move |resource| near(resource.position))
        .filter_map(|resource| LandmarkKind::of_material(resource.resource.kind));
    let built = perception
        .structures
        .iter()
        .filter(move |structure| {
            near(structure.position)
                && structure.kind.landmark()
                && structure.state == crate::StructureState::Complete
                && (structure.kind != crate::StructureKind::Hearth || structure.working(now))
        })
        .map(|structure| LandmarkKind::Structure(structure.kind));
    water.into_iter().chain(sources).chain(built)
}
