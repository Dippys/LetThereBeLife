//! Planning and intentions (docs/plans/THINKING.md): to ease a pressing need,
//! an agent searches backwards through its own cause-and-effect model (never
//! the world) for a short chain of actions, then holds that plan as its one
//! intention, step by step, until the need eases, a step fails, or something
//! more urgent comes up.

use super::habits::Choice;
use super::model::{EFFECTS, Facts, Model};

/// Most steps in a plan.
pub const PLAN_STEPS: usize = 4;
/// Decisions one step may take before it counts as failed.
const STEP_PATIENCE: u8 = 24;
/// The longest plan worth starting, in expected seconds: past that,
/// instinct decides.
const LONGEST_PLAN_SECONDS: u32 = 420;
/// How much better (times) a fresh plan must look to give up the one held.
const MUCH_BETTER: u64 = 2;
/// Expected seconds for an action never timed.
const UNTIMED_SECONDS: u32 = 30;

/// Facts about what's at hand that only a trip brings about (walking past
/// things isn't a way to get them). Fires, huts and stored food are also made.
const FOUND: u16 = Facts::WATER_NEAR | Facts::FOOD_NEAR | Facts::WOOD_NEAR | Facts::STONE_NEAR;

/// The facts that are about being somewhere: reached by a trip to a place
/// it remembers.
const PLACES: u16 = Facts::WATER_NEAR
    | Facts::FOOD_NEAR
    | Facts::WOOD_NEAR
    | Facts::STONE_NEAR
    | Facts::FIRE
    | Facts::DEAD_FIRE
    | Facts::SHELTER
    | Facts::STORE_NEAR;

/// One step of a plan: an action, and the fact it's for (0 for the last
/// step, which eases the need itself). A trip's fact is where it goes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(C)]
pub struct Step {
    /// `Choice as u8 + 1`; 0 marks an empty step.
    choice: u8,
    /// Index into `EFFECTS` + 1; 0 for the last step.
    fact: u8,
}

impl Step {
    pub fn new(choice: Choice, fact: Option<u16>) -> Self {
        Self {
            choice: choice as u8 + 1,
            fact: fact
                .and_then(|fact| EFFECTS.iter().position(|&effect| effect == fact))
                .map_or(0, |index| index as u8 + 1),
        }
    }

    pub fn choice(self) -> Option<Choice> {
        (self.choice != 0).then(|| Choice::ALL[usize::from(self.choice - 1) % Choice::COUNT])
    }

    /// The fact this step brings about, if it isn't the last.
    pub fn fact(self) -> Option<u16> {
        (self.fact != 0).then(|| EFFECTS[usize::from(self.fact - 1) % EFFECTS.len()])
    }
}

/// What an agent is set on doing: 20 bytes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(C)]
pub(crate) struct Intention {
    /// Simulated second it was formed.
    pub(crate) formed: u32,
    steps: [Step; PLAN_STEPS],
    /// The need it eases (`NeedKind` index) + 1; 0 when it intends nothing.
    goal: u8,
    /// The step it's on.
    step: u8,
    /// Decisions spent on this step.
    tries: u8,
    /// Times it has had to give up a plan for this need and think again.
    pub(crate) revisions: u8,
    /// How good it looked when formed (relief per minute, by its chance).
    score: u32,
}

/// An intention as tools and the viewer see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntentionView {
    /// The need it eases, in `NeedKind` order.
    pub need: usize,
    pub steps: [Step; PLAN_STEPS],
    /// The step it's on.
    pub step: usize,
    pub formed: u32,
}

impl Intention {
    /// No intention, after `revisions` plans given up.
    pub(crate) fn none(revisions: u8) -> Self {
        Self {
            revisions,
            ..Self::default()
        }
    }

    pub(crate) const fn goal(self) -> Option<usize> {
        if self.goal == 0 {
            None
        } else {
            Some(self.goal as usize - 1)
        }
    }

    /// The step it's on, if any is left.
    pub(crate) fn current(self) -> Option<Step> {
        self.steps
            .get(usize::from(self.step))
            .copied()
            .filter(|step| step.choice != 0)
    }

    /// Skips the steps whose fact already holds (done, or true anyway).
    pub(crate) fn advance(&mut self, facts: Facts) {
        while let Some(step) = self.current()
            && step.fact().is_some_and(|fact| facts.has(fact))
        {
            self.step += 1;
            self.tries = 0;
        }
    }

    /// One more decision spent on the current step; false once it has taken
    /// too long (the step has failed). A trip is given up only when it stops
    /// getting any closer, which the engine watches.
    pub(crate) fn persist(&mut self) -> bool {
        if self.current().and_then(Step::choice) == Some(Choice::Travel) {
            return true;
        }
        self.tries = self.tries.saturating_add(1);
        self.tries <= STEP_PATIENCE
    }

    pub(crate) fn view(self) -> Option<IntentionView> {
        Some(IntentionView {
            need: self.goal()?,
            steps: self.steps,
            step: usize::from(self.step),
            formed: self.formed,
        })
    }
}

/// What the planner may consider: the actions the agent knows how to do, and
/// the places it could go to.
pub(crate) struct Abilities<'a> {
    pub(crate) can: &'a dyn Fn(Choice) -> bool,
    /// Seconds a trip to a remembered place where the fact holds would take.
    pub(crate) can_reach: &'a dyn Fn(u16) -> Option<u32>,
}

/// A way to bring something about: its steps, expected seconds, and chance
/// it works (thousandths).
#[derive(Debug, Clone)]
struct Way {
    steps: Vec<Step>,
    seconds: u32,
    chance: u32,
}

impl Way {
    fn then(mut self, other: Way) -> Self {
        self.steps.extend(other.steps);
        self.seconds += other.seconds;
        self.chance = self.chance * other.chance / 1_000;
        self
    }
}

/// Searches `model` for the best short plan to ease `need` (`NeedKind`
/// index) from `facts`: an action believed to ease it, preceded by whatever
/// it believes that action needs, each brought about by an action or a trip.
pub(crate) fn plan(
    model: &Model,
    facts: Facts,
    need: usize,
    abilities: &Abilities<'_>,
    revisions: u8,
    now: u32,
) -> Option<Intention> {
    let mut best: Option<(u64, Way)> = None;
    for choice in Choice::ALL {
        let relief = i32::from(model.relief(choice, need));
        if relief <= 0 || !(abilities.can)(choice) {
            continue;
        }
        let Some(before) = prepare(model, facts, model.needs(choice).0, abilities, 2) else {
            continue;
        };
        let last = Way {
            steps: vec![Step::new(choice, None)],
            seconds: duration(model, choice),
            chance: 1_000,
        };
        let way = before.then(last);
        if way.steps.len() > PLAN_STEPS || way.seconds > LONGEST_PLAN_SECONDS {
            continue;
        }
        // Relief per minute, by the chance it all works.
        let score = relief as u64 * u64::from(way.chance) * 60 / u64::from(way.seconds.max(1));
        if best.as_ref().is_none_or(|(top, _)| score > *top) {
            best = Some((score, way));
        }
    }
    let (score, way) = best?;
    let mut steps = [Step::default(); PLAN_STEPS];
    steps[..way.steps.len()].copy_from_slice(&way.steps);
    Some(Intention {
        formed: now,
        steps,
        goal: need as u8 + 1,
        step: 0,
        tries: 0,
        revisions,
        score: score.min(u64::from(u32::MAX)) as u32,
    })
}

impl Intention {
    /// Whether `fresh` is a different plan that looks much better than this
    /// one did: worth changing its mind for.
    pub(crate) fn outdone_by(self, fresh: &Self) -> bool {
        let remaining = &self.steps[usize::from(self.step).min(PLAN_STEPS)..];
        let same = fresh.steps[..remaining.len()] == *remaining;
        !same && u64::from(fresh.score) > MUCH_BETTER * u64::from(self.score)
    }
}

/// The steps that bring about every fact of `wanted` that doesn't hold.
fn prepare(
    model: &Model,
    facts: Facts,
    wanted: u16,
    abilities: &Abilities<'_>,
    depth: u8,
) -> Option<Way> {
    let mut way = Way {
        steps: Vec::new(),
        seconds: 0,
        chance: 1_000,
    };
    let mut have = facts.0;
    for fact in EFFECTS {
        if wanted & fact == 0 || have & fact != 0 {
            continue;
        }
        let found = achieve(model, Facts(have), fact, abilities, depth)?;
        have |= fact;
        way = way.then(found);
    }
    Some(way)
}

/// The best way to bring `fact` about: a trip, or an action believed to
/// produce it (with what that needs, `depth` levels deep).
fn achieve(
    model: &Model,
    facts: Facts,
    fact: u16,
    abilities: &Abilities<'_>,
    depth: u8,
) -> Option<Way> {
    let mut best: Option<Way> = None;
    let mut consider = |way: Way| {
        let better = best.as_ref().is_none_or(|top| {
            u64::from(way.chance) * u64::from(top.seconds.max(1))
                > u64::from(top.chance) * u64::from(way.seconds.max(1))
        });
        if better {
            best = Some(way);
        }
    };
    if fact & PLACES != 0
        && let Some(seconds) = (abilities.can_reach)(fact)
    {
        consider(Way {
            steps: vec![Step::new(Choice::Travel, Some(fact))],
            seconds,
            chance: 800,
        });
    }
    if depth > 0 && fact & FOUND == 0 {
        for choice in Choice::ALL {
            if matches!(choice, Choice::Travel | Choice::Explore) || !(abilities.can)(choice) {
                continue;
            }
            // Something it has seen this do at least half the time.
            let (hits, tries) = model.odds(choice, fact);
            if hits < 2 || hits * 2 < tries {
                continue;
            }
            let Some(before) = prepare(model, facts, model.needs(choice).0, abilities, depth - 1)
            else {
                continue;
            };
            consider(before.then(Way {
                steps: vec![Step::new(choice, Some(fact))],
                seconds: duration(model, choice),
                chance: u32::from(hits) * 1_000 / (u32::from(tries) + 1),
            }));
        }
    }
    best
}

fn duration(model: &Model, choice: Choice) -> u32 {
    match model.seconds(choice) {
        0 => UNTIMED_SECONDS,
        seconds => seconds,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cold_fire_keeper_plans_wood_then_the_fire_then_warming_up() {
        let mut model = Model::instinct();
        model.know_how(true, false, false);
        let facts = Facts(Facts::COLD | Facts::DEAD_FIRE);
        let abilities = Abilities {
            can: &|_| true,
            can_reach: &|fact| (fact == Facts::WOOD_NEAR).then_some(60),
        };
        let steps = |intention: Intention| -> Vec<(Option<Choice>, Option<u16>)> {
            intention
                .steps
                .iter()
                .filter(|step| step.choice != 0)
                .map(|step| (step.choice(), step.fact()))
                .collect()
        };
        let first = plan(&model, facts, 3, &abilities, 0, 100).expect("a plan");
        assert_eq!(
            steps(first),
            [
                (Some(Choice::Gather), Some(Facts::HAS_WOOD)),
                (Some(Choice::TendFire), Some(Facts::FIRE)),
                (Some(Choice::WarmUp), None),
            ]
        );
        // Nothing to gather where it stood, three times: gathering needs wood at hand.
        for _ in 0..3 {
            model.lacked(Choice::Gather, Facts::WOOD_NEAR);
        }
        let intention = plan(&model, facts, 3, &abilities, 1, 200).expect("a plan");
        assert_eq!(
            steps(intention),
            [
                (Some(Choice::Travel), Some(Facts::WOOD_NEAR)),
                (Some(Choice::Gather), Some(Facts::HAS_WOOD)),
                (Some(Choice::TendFire), Some(Facts::FIRE)),
                (Some(Choice::WarmUp), None),
            ]
        );
        let mut intention = intention;
        intention.advance(Facts(facts.0 | Facts::WOOD_NEAR | Facts::HAS_WOOD));
        assert_eq!(
            intention.current().and_then(Step::choice),
            Some(Choice::TendFire)
        );
        assert_eq!(std::mem::size_of::<Intention>(), 20);
    }

    #[test]
    fn without_the_know_how_or_a_way_there_is_no_plan() {
        let model = Model::instinct();
        let abilities = Abilities {
            can: &|_| true,
            can_reach: &|_| None,
        };
        assert!(plan(&model, Facts(Facts::COLD), 3, &abilities, 0, 0).is_none());
        let plan = plan(&model, Facts(Facts::THIRSTY), 1, &abilities, 0, 0);
        assert!(plan.is_none(), "no water at hand, none remembered");
    }
}
