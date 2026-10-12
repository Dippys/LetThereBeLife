//! Agent cognition: private mental maps (remembered places, explored tiles),
//! sparse relationships, personality, and pointing gestures that let agents pass
//! knowledge to each other through observable behavior. Beliefs live here;
//! physical truth stays in the world, population, and resource stores.

mod affordances;
mod character;
mod crafts;
mod dialogue;
mod fauna;
mod gesture;
mod habits;
mod intention;
mod lexicon;
mod map;
mod model;
mod personality;
mod reading;
mod signal;
mod social;

pub use affordances::AffordanceView;
pub(crate) use affordances::Affordances;
pub use character::DRIFT_LIMIT;
pub(crate) use character::{Character, by_ability};
pub(crate) use crafts::{Attempt, Crafts, Design};
pub use dialogue::{CONSEQUENCE_WEIGHT, REPAIR_WEIGHT};
pub(crate) use dialogue::{Dialogue, LEAD_SECONDS, Lead, PendingCorrection};
pub(crate) use fauna::Fauna;
pub use fauna::FaunaView;
pub use gesture::Gesture;
pub(crate) use gesture::reach_toward;
pub use habits::{Choice, EFFECTS, EffectView, HABIT_SLOTS, HabitView, Situation};
pub(crate) use habits::{Habits, discomfort};
pub(crate) use intention::{Abilities, Intention, plan, plan_ready};
pub use intention::{Goal, IntentionView, PLAN_STEPS, Step};
pub(crate) use lexicon::Lexicon;
pub use lexicon::{Concept, FAMILY_SIZE, LEXICON_SLOTS, LexiconEntryView, VOCAL_FORMS, VocalForm};
pub(crate) use map::{HintCheck, HintSource, MentalMap, spent_kinds, visible_kinds};
pub use map::{LANDMARK_SLOTS, MERGE_RADIUS, SEARCH_SPACING, VISIT_TILE_SIZE, VISITED_TILE_SLOTS};
pub use model::{BeliefView, Facts};
pub(crate) use model::{EFFECTS as MADE, Model, need_level};
pub use personality::{Activity, Personality};
pub(crate) use reading::ListenerContext;
pub use reading::{READING_CANDIDATES, Reading, ReadingReasons, concept_topic};
pub use signal::{DesiredEffect, Mime, PublicSignal, Tone, Understanding, UtteranceIntent};
pub(crate) use signal::{express, locate, mime_for, understand, unmistakable};
pub(crate) use social::SocialMemory;
pub use social::{
    ACQUAINTANCE_SLOTS, AcquaintanceView, DEFAULT_TRUST, DISTRUST, FRIEND_FAMILIARITY, Tie,
};

use crate::{AgentId, SimTime, WorldPosition};

/// Seconds-resolution timestamps keep remembered places at 12 bytes.
pub(crate) fn belief_seconds(time: SimTime) -> u32 {
    u32::try_from(time.ticks() / 60).unwrap_or(u32::MAX - 1)
}

/// Simulated seconds between two gestures by an agent of average sociability.
/// The actual cooldown runs from 110 s (reserved) down to 10 s (chatty).
pub const SHARE_COOLDOWN_SECONDS: u32 = 60;

/// How sure a watcher is of a hint, given how much it trusts the teller.
/// Strangers start at `DEFAULT_TRUST`, which gives 144 (a first-hand sighting is 255).
pub(crate) const fn told_confidence(trust: u8) -> u8 {
    48 + (trust as u16 * 3 / 4) as u8
}
/// How long a pointing gesture takes, in ticks.
pub const SIGNAL_TICKS: u64 = 120;

/// A kind of place worth remembering: water, wherever a material is found at a
/// fixed source, and each kind of structure. Derived from the world's tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LandmarkKind {
    Water,
    Material(crate::Material),
    Structure(crate::StructureKind),
}

/// Materials found at fixed places, in material order.
const FIXED_MATERIALS: usize = {
    let mut count = 0;
    let mut index = 0;
    while index < crate::Material::COUNT {
        if crate::Material::ALL[index].properties().fixed_source {
            count += 1;
        }
        index += 1;
    }
    count
};

/// Kinds of structure worth remembering where they are.
const LANDMARK_STRUCTURES: usize = {
    let mut count = 0;
    let mut index = 0;
    while index < crate::StructureKind::COUNT {
        if crate::StructureKind::ALL[index].landmark() {
            count += 1;
        }
        index += 1;
    }
    count
};

impl LandmarkKind {
    pub const COUNT: usize = 1 + FIXED_MATERIALS + LANDMARK_STRUCTURES;
    pub const ALL: [Self; Self::COUNT] = {
        let mut all = [Self::Water; Self::COUNT];
        let mut next = 1;
        let mut index = 0;
        while index < crate::Material::COUNT {
            let material = crate::Material::ALL[index];
            if material.properties().fixed_source {
                all[next] = Self::Material(material);
                next += 1;
            }
            index += 1;
        }
        index = 0;
        while index < crate::StructureKind::COUNT {
            let kind = crate::StructureKind::ALL[index];
            if kind.landmark() {
                all[next] = Self::Structure(kind);
                next += 1;
            }
            index += 1;
        }
        all
    };
    pub const BERRIES: Self = Self::Material(crate::Material::Berries);
    pub const BITTERBERRIES: Self = Self::Material(crate::Material::Bitterberries);
    pub const WOOD: Self = Self::Material(crate::Material::Wood);
    pub const STONE: Self = Self::Material(crate::Material::Stone);
    pub const SHELTER: Self = Self::Structure(crate::StructureKind::Shelter);
    pub const HEARTH: Self = Self::Structure(crate::StructureKind::Hearth);

    /// Position in `ALL`.
    pub const fn index(self) -> usize {
        match self {
            Self::Water => 0,
            Self::Material(material) => {
                let mut position = 1;
                let mut index = 0;
                while index < material as usize {
                    if crate::Material::ALL[index].properties().fixed_source {
                        position += 1;
                    }
                    index += 1;
                }
                position
            }
            Self::Structure(kind) => 1 + FIXED_MATERIALS + kind as usize,
        }
    }

    /// The kind of place where `material` can be gathered, if it stays put
    /// (meat lies on carcasses that soon spoil, so nobody remembers them).
    pub const fn of_material(material: crate::Material) -> Option<Self> {
        if material.properties().fixed_source {
            Some(Self::Material(material))
        } else {
            None
        }
    }

    /// The material gathered at this kind of place, if any.
    pub const fn material(self) -> Option<crate::Material> {
        match self {
            Self::Material(material) => Some(material),
            Self::Water | Self::Structure(_) => None,
        }
    }

    /// The structure this kind of place is, if any.
    pub const fn structure(self) -> Option<crate::StructureKind> {
        match self {
            Self::Structure(kind) => Some(kind),
            Self::Water | Self::Material(_) => None,
        }
    }

    /// What people think of this kind of place as.
    pub const fn concept(self) -> Concept {
        match self {
            Self::Water => Concept::Water,
            Self::Material(material) => Concept::Material(material),
            Self::Structure(kind) => Concept::Structure(kind),
        }
    }
}

/// How an agent came to believe in a place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LandmarkSource {
    /// It saw the place itself.
    Seen,
    /// It inferred the place from someone's pointing gesture.
    Told,
}

/// A read-only copy of one remembered place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LandmarkView {
    pub kind: LandmarkKind,
    pub position: WorldPosition,
    pub source: LandmarkSource,
    /// 255 for a fresh first-hand sighting; lower for hearsay and failed searches.
    pub confidence: u8,
    /// Cells around `position` the agent expects to search (0 when seen first-hand).
    pub search_radius: u16,
    /// Simulated second when last confirmed or heard.
    pub seen_second: u32,
}

/// A read-only copy of what an agent knows and who it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MentalMapView {
    pub agent: AgentId,
    pub personality: Personality,
    /// How good it is at each `Activity` (0-255): talent and practice.
    pub abilities: [u8; Activity::COUNT],
    /// Its practised skill at each `Activity` (0-255).
    pub skills: [u8; Activity::COUNT],
    pub landmarks: Vec<LandmarkView>,
    pub explored_tiles: usize,
    /// A child of the band (started with no words) rather than a founder.
    pub child: bool,
    /// What it believes materials are good for (only materials it has beliefs about).
    pub affordances: Vec<AffordanceView>,
    /// What it believes about animals (only species it has beliefs about).
    pub fauna: Vec<FaunaView>,
    /// Believes a hearth would warm it (and knows how to build one).
    pub knows_hearths: bool,
    /// Knows how to knap a stone blade.
    pub knows_knapping: bool,
    /// Knows how to build a hut.
    pub knows_huts: bool,
    /// How its own choices have gone, in each kind of situation.
    pub habits: Vec<HabitView>,
    /// What it has seen its kinds of choice bring about.
    pub effects: Vec<EffectView>,
    /// The hut it lives in.
    pub home: Option<WorldPosition>,
    /// What it believes its kinds of action need and bring about.
    pub beliefs: Vec<BeliefView>,
    pub acquaintances: Vec<AcquaintanceView>,
    /// What the agent believes words mean.
    pub lexicon: Vec<LexiconEntryView>,
}

/// What a gesture was about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureTopic {
    /// "There is water / food / ... over there."
    Place(LandmarkKind),
    /// "I've already been over there" (watchers treat that ground as explored).
    Explored,
    /// "There's a deer / wolf over there": a call to hunt or a warning.
    Animal(crate::Species),
}

/// One gesture completed during the latest tick, for logs and tools only.
/// Agents never see this record: receivers get just the public `gesture`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignalEvent {
    /// Unique, increasing per engine run (reset clears it).
    pub id: u64,
    pub at: SimTime,
    /// **Private** to the sender: what it meant and the exact place it had in mind.
    pub intent: UtteranceIntent,
    /// **Public:** everything anyone watching could see.
    pub signal: PublicSignal,
    /// Where watchers concluded the place is (what they can know, not the truth).
    pub inferred_position: WorldPosition,
    /// Search radius watchers attach to that conclusion.
    pub search_radius: u16,
    /// Watchers whose mental map changed.
    pub informed: u16,
    /// Awake agents that saw the gesture.
    pub watchers: u16,
}

/// How one watcher read one gesture (latest tick, for logs and tools only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterpretationEvent {
    pub signal: u64,
    pub receiver: AgentId,
    pub at: SimTime,
    /// What the receiver took the gesture to be about.
    pub understood: GestureTopic,
    pub estimate: WorldPosition,
    pub search_radius: u16,
    /// Confidence given to the hint (0 for "explored" gestures).
    pub confidence: u8,
    /// Whether the receiver's beliefs changed.
    pub changed: bool,
    /// The word the receiver heard, if the sender said one.
    pub heard: Option<VocalForm>,
    /// What the receiver thought that word meant *before* learning from this signal.
    pub word_reading: Option<Concept>,
    /// The competing meanings the receiver weighed, and why it chose as it did.
    pub reading: Reading,
}

/// Why an agent changed what it believes a word means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LessonCause {
    /// It went to a place it was told about and saw what was (or wasn't) there.
    Consequence,
    /// It asked "this?" and the speaker nodded.
    Confirmation,
    /// It asked "this?" and the speaker repeated with a clearer mime.
    Repair,
    /// Someone pointed back at a place, said a word, and mimed "not this, that".
    Correction,
    /// It heard someone use a word for something other than what it thought the
    /// word meant (learning from ordinary use).
    Usage,
}

/// One change to an agent's lexicon or word habits (latest tick, for logs only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LessonEvent {
    pub agent: AgentId,
    pub at: SimTime,
    pub form: VocalForm,
    /// The meaning the agent now favors more, if any.
    pub strengthened: Option<Concept>,
    /// The meaning the agent now doubts, if any.
    pub weakened: Option<Concept>,
    /// For speakers: whether its use of the word was judged to have worked.
    pub use_worked: Option<bool>,
    pub cause: LessonCause,
    /// The gesture this lesson traces back to (the tip, the question, or the correction).
    pub signal: Option<u64>,
}

/// How a speaker answered a listener's "this?" (latest tick, for logs only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairResponse {
    /// A nod: the guess was right.
    Confirmed,
    /// A clearer, exaggerated mime of what was meant.
    Repaired(Concept),
}

/// How someone who was asked for food answered (all of it visible to the asker).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestResponse {
    /// Handed over food and nodded.
    Gave,
    /// Shook its head while holding food.
    Refused,
    /// Showed empty hands.
    NothingToGive,
}

/// A request for food and its answer (latest tick, for logs and tools only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestEvent {
    pub signal: u64,
    pub asker: AgentId,
    pub giver: AgentId,
    pub at: SimTime,
    /// What the giver first took the request to be about (before any repair).
    pub read_as: Concept,
    pub response: RequestResponse,
    /// What was handed over, if anything (the giver's idea of food).
    pub given: Option<crate::Material>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepairEvent {
    pub signal: u64,
    pub listener: AgentId,
    pub at: SimTime,
    /// What the listener mimed back.
    pub guess: Concept,
    /// The listener's own word for its guess, said with the question.
    pub listener_word: Option<VocalForm>,
    pub response: RepairResponse,
}

/// Someone acted on a tip about an animal: ran from a warned-about spot, or
/// went after an animal someone pointed out (latest tick, for logs and tools).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeadFollowedEvent {
    pub agent: AgentId,
    pub at: SimTime,
    pub signal: u64,
    pub species: crate::Species,
    /// Ran from it (a warning) rather than going after it.
    pub fled: bool,
}

/// A word changed hands in a new way (latest tick, for logs and tools).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WordEvent {
    /// Someone made up a word for something it had no word for.
    Coined {
        agent: AgentId,
        form: VocalForm,
        concept: Concept,
    },
    /// A child picked up a word with a vowel changed.
    Shifted {
        agent: AgentId,
        heard: VocalForm,
        learned: VocalForm,
    },
}

/// Someone noticed a bush where it had eaten berries (and learned that seeds
/// grow), or planted one (latest tick, for logs and tools).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FarmEvent {
    pub agent: AgentId,
    pub at: WorldPosition,
    /// It planted on purpose (otherwise, it noticed one had come up).
    pub planted: bool,
    /// How many saw it planted (and learned how).
    pub watchers: u16,
}

/// Someone made something (latest tick, for logs and tools).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CraftEvent {
    pub agent: AgentId,
    pub made: crate::Material,
    /// How many saw it done (and learned how).
    pub watchers: u16,
}

/// Someone put fuel on a fire (latest tick, for logs and tools).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FireEvent {
    pub agent: AgentId,
    pub at: WorldPosition,
    /// The fire had gone out and was lit again.
    pub relit: bool,
}

/// Two people became a couple (latest tick, for logs and tools).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoupleEvent {
    pub first: AgentId,
    pub second: AgentId,
    pub at: crate::SimTime,
}

/// Someone saw the body of a person close to them and is mourning (latest
/// tick, for logs and tools).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GriefEvent {
    pub agent: AgentId,
    pub lost: AgentId,
    pub at: crate::SimTime,
}

/// What became of someone's intention this decision (latest tick, for logs,
/// the viewer and tools).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanEventKind {
    /// It thought of a plan.
    Formed,
    /// The need it was for eased.
    Completed,
    /// A step failed and it gave the plan up (it may form another).
    Revised,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanEvent {
    pub agent: AgentId,
    pub at: crate::SimTime,
    pub kind: PlanEventKind,
    /// The intention formed, completed, or given up.
    pub intention: IntentionView,
}

/// Someone ate something (latest tick, for logs and tools only). Eating and
/// retching are visible, so `watchers` saw it and learned from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MealEvent {
    pub agent: AgentId,
    pub at: SimTime,
    pub material: crate::Material,
    /// It made the eater sick (visibly).
    pub retched: bool,
    /// The eater had never tried it before.
    pub first_taste: bool,
    pub watchers: u16,
}

/// A hint that was checked by looking (latest tick, for logs and tools only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HintOutcomeEvent {
    pub agent: AgentId,
    /// Who pointed it out, if still remembered.
    pub teller: Option<AgentId>,
    pub kind: LandmarkKind,
    /// `true`: found what the hint promised; `false`: searched and gave up.
    pub confirmed: bool,
    pub at: SimTime,
}

/// The needs met by planning (docs/plans/THINKING.md): bit `1 << NeedKind
/// index` set means that need is eased by a plan from the agent's own
/// cause-and-effect model, held as an intention; bit 4 gets ready for a hard
/// season it remembers coming (built but off, D-120); bit 5 spends idle time
/// getting what has proved useful (D-123).
pub(crate) const PLANS: u8 = 1 << crate::NeedKind::Hunger as u8
    | 1 << crate::NeedKind::Thirst as u8
    | 1 << crate::NeedKind::Exposure as u8
    | 1 << 5;

impl GestureTopic {
    /// The concept a topic is about.
    pub const fn concept(self) -> Concept {
        match self {
            Self::Place(kind) => kind.concept(),
            Self::Explored => Concept::Been,
            Self::Animal(species) => Concept::Species(species),
        }
    }
}

/// Everything one agent privately knows: places, people, and words.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Mind {
    pub(crate) map: MentalMap,
    pub(crate) social: SocialMemory,
    pub(crate) lexicon: Lexicon,
    pub(crate) dialogue: Dialogue,
    /// What it believes materials are good for.
    pub(crate) affordances: Affordances,
    /// What it believes about animals.
    pub(crate) fauna: Fauna,
    /// What it knows about things people make.
    pub(crate) crafts: Crafts,
    /// Born into the band rather than founding it: starts with no words, stays
    /// close to its parent, and asks readily.
    pub(crate) child: bool,
    /// The agent a child stays close to.
    pub(crate) parent: Option<AgentId>,
    /// Mourning someone close until this simulated second.
    pub(crate) grief_until: u32,
    /// Simulated second it last saw someone it could pair with.
    pub(crate) last_eligible_seen: u32,
    /// The hut it last put food away in, until it finds it empty.
    pub(crate) stored_food: Option<crate::agent::CompactPosition>,
    /// How close it has got to where it's headed, to give up on places it
    /// can't reach.
    pub(crate) stall: Stall,
    /// How its own choices have gone (docs/plans/LEARNING.md).
    pub(crate) habits: Habits,
    /// The hut it lives in.
    pub(crate) home: Home,
    /// What it believes its actions need and bring about.
    pub(crate) model: Model,
    /// What it is set on doing.
    pub(crate) intention: Intention,
    /// A place it recently couldn't get to.
    pub(crate) avoid: Avoid,
    /// How hard each season was, the last time it lived through it.
    pub(crate) seasons: SeasonMemory,
    /// Where it last ate berries on open ground.
    pub(crate) sown: Sown,
    /// Simulated second before which it won't fall sick by a body again.
    pub(crate) sick_until: u32,
    /// Skills from practice, and how life has shifted its traits.
    pub(crate) character: Character,
}

/// Seconds after eating somewhere before a bush there counts as having come
/// up from it, and after which the spot is forgotten.
const SOWN_WAIT: u32 = 600;
const SOWN_FORGET: u32 = crate::SECONDS_PER_YEAR as u32;

/// Where it last ate berries with no bush near, and when: if a bush bearing
/// berries stands there later, it has seen seeds grow.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Sown {
    spot: Option<crate::agent::CompactPosition>,
    at: u32,
}

impl Sown {
    /// It ate berries at `position` with no bush near (kept until it learns
    /// something from the spot, or forgets it).
    pub(crate) fn ate_at(&mut self, position: WorldPosition, now: u32) {
        if self.spot.is_none() || now.saturating_sub(self.at) > SOWN_FORGET {
            self.spot = crate::agent::CompactPosition::checked(position);
            self.at = now;
        }
    }

    /// Whether a bush bearing berries now stands within a cell of the spot,
    /// a while after it ate there: seeds grow.
    pub(crate) fn sprouted(
        &mut self,
        mut bushes: impl Iterator<Item = WorldPosition>,
        now: u32,
    ) -> bool {
        let Some(spot) = self.spot.map(crate::agent::CompactPosition::world) else {
            return false;
        };
        if now.saturating_sub(self.at) < SOWN_WAIT {
            return false;
        }
        let grew = bushes.any(|bush| bush.x.abs_diff(spot.x) + bush.y.abs_diff(spot.y) <= 1);
        if grew {
            self.spot = None;
        }
        grew
    }
}

/// How cold and how hungry it got in each season the last time it came round
/// (hundredths of the need's threshold, so 100 means it pressed), and this
/// season so far.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct SeasonMemory {
    last: [[u8; 2]; 4],
    this: [u8; 2],
    /// The season `this` is about, + 1 (0 before it has felt any).
    season: u8,
}

impl SeasonMemory {
    /// It feels `cold` and `hungry` (need levels) in `season` (index).
    pub(crate) fn felt(&mut self, season: usize, cold: u8, hungry: u8) {
        let season = season as u8 % 4 + 1;
        if self.season != season {
            if self.season != 0 {
                self.last[usize::from(self.season - 1)] = self.this;
            }
            self.this = [0, 0];
            self.season = season;
        }
        self.this = [self.this[0].max(cold), self.this[1].max(hungry)];
    }

    /// How cold and how hungry `season` (index) got last time.
    pub(crate) fn remembered(self, season: usize) -> [u8; 2] {
        self.last[season % 4]
    }
}

/// Seconds a place it couldn't get to is kept clear of.
pub const AVOID_SECONDS: u32 = 600;

/// A place an agent recently couldn't get to, and until when it keeps clear.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Avoid {
    place: Option<crate::agent::CompactPosition>,
    until: u32,
}

impl Avoid {
    pub(crate) fn of(place: WorldPosition, now: u32) -> Self {
        Self {
            place: crate::agent::CompactPosition::checked(place),
            until: now.saturating_add(AVOID_SECONDS),
        }
    }

    /// The place, while it's still avoided at `now`.
    pub(crate) fn place(self, now: u32) -> Option<WorldPosition> {
        self.place
            .filter(|_| now < self.until)
            .map(crate::agent::CompactPosition::world)
    }
}

/// The most nights of attachment a home builds up.
pub const HOME_ATTACHMENT_MAX: u8 = 16;

/// The hut an agent lives in, and how attached it is: each night under its
/// roof adds to that, each night elsewhere wears it down, and once it's worn
/// away the next hut it sleeps in becomes home.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Home {
    hut: Option<crate::agent::CompactPosition>,
    attachment: u8,
}

impl Home {
    pub(crate) fn hut(self) -> Option<WorldPosition> {
        self.hut.map(crate::agent::CompactPosition::world)
    }

    /// A night under the roof of the hut at `hut`. A night far from home
    /// wears attachment down faster: a few of those and it has moved on.
    pub(crate) fn slept_at(&mut self, hut: WorldPosition, far: bool) {
        let hut = crate::agent::CompactPosition::checked(hut);
        if self.hut == hut {
            self.attachment = (self.attachment + 1).min(HOME_ATTACHMENT_MAX);
        } else if self.attachment <= Self::wear(far) {
            self.hut = hut;
            self.attachment = 1;
        } else {
            self.attachment -= Self::wear(far);
        }
    }

    /// A night away from any roof.
    pub(crate) fn slept_out(&mut self, far: bool) {
        self.attachment = self.attachment.saturating_sub(Self::wear(far));
        if self.attachment == 0 {
            self.hut = None;
        }
    }

    const fn wear(far: bool) -> u8 {
        if far { 4 } else { 1 }
    }

    /// Whether `position` is far from home (no home is never far).
    pub(crate) fn far_from(self, position: WorldPosition) -> bool {
        self.hut().is_some_and(|hut| {
            hut.x.abs_diff(position.x) + hut.y.abs_diff(position.y) > crate::HOME_RANGE
        })
    }

    /// It saw its home is gone.
    pub(crate) fn lost(&mut self) {
        *self = Self::default();
    }
}

/// Progress toward one destination: the closest it has been, and how many
/// decisions since it last got closer.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Stall {
    pub(crate) target: Option<crate::agent::CompactPosition>,
    pub(crate) best: u16,
    pub(crate) since_closer: u8,
}

/// Decisions without getting closer before a destination is given up.
pub(crate) const STALL_DECISIONS: u8 = 16;

impl Stall {
    /// Notes the distance to `target` now; true when it's time to give up on it.
    pub(crate) fn note(&mut self, target: WorldPosition, distance: u64) -> bool {
        let distance = u16::try_from(distance).unwrap_or(u16::MAX);
        let compact = crate::agent::CompactPosition::checked(target);
        if self.target != compact {
            *self = Self {
                target: compact,
                best: distance,
                since_closer: 0,
            };
            return false;
        }
        if distance < self.best {
            self.best = distance;
            self.since_closer = 0;
            return false;
        }
        self.since_closer = self.since_closer.saturating_add(1);
        if self.since_closer >= STALL_DECISIONS {
            *self = Self::default();
            return true;
        }
        false
    }
}

impl Mind {
    /// Registers `other` as seen; keeps hint sources consistent if someone was forgotten.
    pub(crate) fn notice(
        &mut self,
        other: AgentId,
        position: WorldPosition,
        now: u32,
    ) -> Option<u8> {
        let noticed = self.social.notice(other, position, now)?;
        if noticed.evicted {
            self.map.forget_teller(noticed.slot);
        }
        Some(noticed.slot)
    }
}

/// Minds for every agent, indexed by `AgentId`, grown on demand. Agents below
/// `founders` inherit the seed's noisy proto-language; later ids are children
/// who start with no words.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Minds {
    seed: u64,
    founders: u32,
    /// Later arrivals who bring their own family's lore and words (sorted).
    newcomers: Vec<u32>,
    minds: Vec<Mind>,
}

impl Default for Minds {
    fn default() -> Self {
        Self::new(0)
    }
}

impl Minds {
    pub(crate) fn new(seed: u64) -> Self {
        Self {
            seed,
            founders: u32::MAX,
            newcomers: Vec::new(),
            minds: Vec::new(),
        }
    }

    /// Agents with ids at or above `count` are children. Affects minds created later.
    /// A founder (present from the start with the band's lore) rather than a child.
    pub(crate) fn is_founder(&self, agent: AgentId) -> bool {
        agent.get() < self.founders || self.newcomers.binary_search(&agent.get()).is_ok()
    }

    pub(crate) fn is_newcomer(&self, agent: AgentId) -> bool {
        self.newcomers.binary_search(&agent.get()).is_ok()
    }

    /// `agent` arrived from elsewhere with its own family's lore and words.
    pub(crate) fn add_newcomer(&mut self, agent: AgentId) {
        if let Err(index) = self.newcomers.binary_search(&agent.get()) {
            self.newcomers.insert(index, agent.get());
        }
    }

    pub(crate) fn set_founders(&mut self, count: u32) {
        self.founders = count;
    }

    pub(crate) fn get(&self, agent: AgentId) -> Option<&Mind> {
        self.minds.get(agent.get() as usize)
    }

    pub(crate) fn get_mut(&mut self, agent: AgentId) -> &mut Mind {
        let index = agent.get() as usize;
        while self.minds.len() <= index {
            let mind = self.newborn(AgentId::new(self.minds.len() as u32));
            self.minds.push(mind);
        }
        &mut self.minds[index]
    }

    /// What `agent` believes materials are good for, whether or not its mind has
    /// been created yet.
    pub(crate) fn affordances(&self, agent: AgentId) -> Affordances {
        self.get(agent)
            .map_or_else(|| self.newborn(agent).affordances, |mind| mind.affordances)
    }

    /// The mind `agent` starts with: a founder inherits the seed's proto-language
    /// and its family's food culture; a child starts with neither.
    fn newborn(&self, agent: AgentId) -> Mind {
        let child = !self.is_founder(agent);
        Mind {
            lexicon: if child {
                Lexicon::default()
            } else {
                Lexicon::founding(self.seed, agent)
            },
            affordances: if child {
                Affordances::default()
            } else {
                Affordances::founding(self.seed, agent, FAMILY_SIZE)
            },
            fauna: if child {
                Fauna::default()
            } else {
                Fauna::founding(self.seed, agent, FAMILY_SIZE)
            },
            crafts: if child {
                Crafts::default()
            } else {
                Crafts::founding(self.seed, agent, FAMILY_SIZE)
            },
            child,
            model: Model::instinct(),
            character: if child {
                Character::default()
            } else {
                Character::seasoned(&Personality::of(self.seed, agent))
            },
            ..Mind::default()
        }
    }
}

#[cfg(test)]
mod tests;
