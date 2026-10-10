//! Agent cognition: private mental maps (remembered places, explored tiles),
//! sparse relationships, personality, and pointing gestures that let agents pass
//! knowledge to each other through observable behavior. Beliefs live here;
//! physical truth stays in the world, population, and resource stores.

mod affordances;
mod crafts;
mod dialogue;
mod fauna;
mod gesture;
mod lexicon;
mod map;
mod personality;
mod reading;
mod signal;
mod social;

pub(crate) use affordances::Affordances;
pub use affordances::{AffordanceView, BELIEF_UNIT};
pub(crate) use crafts::Crafts;
pub use dialogue::{CONSEQUENCE_WEIGHT, REPAIR_WEIGHT};
pub(crate) use dialogue::{Dialogue, LEAD_SECONDS, Lead, PendingCorrection};
pub(crate) use fauna::Fauna;
pub use fauna::FaunaView;
pub use gesture::Gesture;
pub(crate) use gesture::reach_toward;
pub(crate) use lexicon::Lexicon;
pub use lexicon::{Concept, FAMILY_SIZE, LEXICON_SLOTS, LexiconEntryView, VOCAL_FORMS, VocalForm};
pub(crate) use map::{HintCheck, HintSource, MentalMap, spent_kinds, visible_kinds};
pub use map::{LANDMARK_SLOTS, MERGE_RADIUS, SEARCH_SPACING, VISIT_TILE_SIZE, VISITED_TILE_SLOTS};
pub use personality::Personality;
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

impl LandmarkKind {
    pub const COUNT: usize = 1 + FIXED_MATERIALS + crate::StructureKind::COUNT;
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
            all[next] = Self::Structure(crate::StructureKind::ALL[index]);
            next += 1;
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

/// Which cognitive features the autonomous policy uses.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PolicyOptions {
    /// Wander when nothing useful is in view (legacy viewer behavior).
    pub exploration: bool,
    /// Remember places, explore unvisited areas, travel to remembered places,
    /// and plan trips around known water.
    pub memory: bool,
    /// Point out remembered places to nearby agents (requires `memory`).
    pub sharing: bool,
    /// Individual personalities, relationships, visiting friends, and trust-
    /// weighted hints (requires `memory`). Without it everyone is average.
    pub social: bool,
    /// Hungry agents ask others for food, who may give some (requires `social`).
    pub helping: bool,
}

impl PolicyOptions {
    /// Every cognitive feature: the full current agent mind.
    pub const fn full() -> Self {
        Self {
            exploration: true,
            memory: true,
            sharing: true,
            social: true,
            helping: true,
        }
    }
}

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
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug)]
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
            ..Mind::default()
        }
    }
}

#[cfg(test)]
mod tests;
