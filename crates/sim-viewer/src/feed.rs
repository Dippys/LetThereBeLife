//! Presentation-only feed of notable happenings (deaths, bites, kills, help,
//! warnings, misunderstandings), copied from the engine's per-tick logs.

use std::collections::VecDeque;

use sim_core::{
    AgentId, DesiredEffect, Engine, FamilyEvent, GestureTopic, HealthDiagnosticKind, LessonCause,
    Mime, Sex, WildlifeEvent, WorldPosition,
};

use crate::labels;

/// Entries kept; older ones are dropped first.
pub const FEED_CAPACITY: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Someone was hurt, got sick, or died.
    Bad,
    /// A kill or a gift.
    Good,
    /// Talk: warnings, calls, misunderstandings, corrections.
    Talk,
    Neutral,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedEntry {
    pub text: String,
    pub tone: Tone,
    /// Who it is mostly about (clicking the entry selects them).
    pub agent: Option<AgentId>,
    pub position: WorldPosition,
    /// How many times in a row this same thing happened.
    pub count: u32,
}

/// Story entries kept; the oldest are dropped first.
pub const STORY_CAPACITY: usize = 4_000;

/// Something worth remembering in the band's story, and the year it happened in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryEntry {
    /// 1 for the first year.
    pub year: u32,
    pub entry: FeedEntry,
}

#[derive(Debug, Default)]
pub struct Feed {
    entries: VecDeque<FeedEntry>,
    /// The band's story: births, deaths, couples, newcomers, firsts.
    story: VecDeque<StoryEntry>,
    deaths_seen: usize,
    births: u32,
    /// Huts and hearths finished so far (the first of each is in the story).
    built: [u32; 2],
    /// Who started each structure being built (by structure id).
    builders: std::collections::BTreeMap<u32, AgentId>,
}

impl Feed {
    /// Oldest first.
    pub fn entries(&self) -> impl ExactSizeIterator<Item = &FeedEntry> {
        self.entries.iter()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.story.clear();
        self.deaths_seen = 0;
        self.births = 0;
        self.built = [0, 0];
        self.builders.clear();
    }

    /// The band's story so far, oldest first.
    pub fn story(&self) -> impl DoubleEndedIterator<Item = &StoryEntry> {
        self.story.iter()
    }

    /// A line in the recent events that also goes into the band's story.
    fn tell(
        &mut self,
        engine: &Engine,
        text: String,
        tone: Tone,
        agent: Option<AgentId>,
        position: WorldPosition,
    ) {
        if self.story.len() == STORY_CAPACITY {
            self.story.pop_front();
        }
        self.story.push_back(StoryEntry {
            year: year_of(engine),
            entry: FeedEntry {
                text: text.clone(),
                tone,
                agent,
                position,
                count: 1,
            },
        });
        self.push(text, tone, agent, position);
    }

    /// Babies born since the feed started.
    pub const fn births(&self) -> u32 {
        self.births
    }

    fn push(&mut self, text: String, tone: Tone, agent: Option<AgentId>, position: WorldPosition) {
        if let Some(last) = self.entries.back_mut()
            && last.text == text
        {
            last.count += 1;
            last.position = position;
            return;
        }
        if self.entries.len() == FEED_CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back(FeedEntry {
            text,
            tone,
            agent,
            position,
            count: 1,
        });
    }

    /// Reads what happened during the engine's latest tick.
    pub fn record(&mut self, engine: &Engine) {
        let deaths = engine.death_records();
        self.deaths_seen = self.deaths_seen.min(deaths.len());
        for started in engine
            .structure_diagnostics()
            .iter()
            .filter(|diagnostic| diagnostic.kind == sim_core::StructureDiagnosticKind::Started)
        {
            if let Some(builder) = started.structure.builder {
                self.builders.insert(started.structure.id.get(), builder);
            }
        }
        let fresh: Vec<_> = deaths[self.deaths_seen..].to_vec();
        for death in &fresh {
            self.tell(
                engine,
                format!(
                    "{} {}",
                    name(engine, death.agent),
                    labels::death(death.cause)
                ),
                Tone::Bad,
                Some(death.agent),
                death.position,
            );
        }
        self.deaths_seen = deaths.len();

        for health in engine.health_diagnostics() {
            let text = match health.kind {
                HealthDiagnosticKind::Incapacitated => "collapsed",
                HealthDiagnosticKind::Recovered => "got back on their feet",
                _ => continue,
            };
            let tone = if health.kind == HealthDiagnosticKind::Recovered {
                Tone::Good
            } else {
                Tone::Bad
            };
            self.push(
                format!("{} {text}", name(engine, health.agent)),
                tone,
                Some(health.agent),
                agent_position(engine, health.agent),
            );
        }

        self.births += engine
            .family_events()
            .iter()
            .filter(|event| matches!(event, FamilyEvent::Born { .. }))
            .count() as u32;
        for event in engine.family_events() {
            let (text, tone, agent) = match *event {
                FamilyEvent::Born { mother, sex, .. } => (
                    format!(
                        "{} gave birth to a {}",
                        name(engine, mother),
                        match sex {
                            Sex::Female => "girl",
                            Sex::Male => "boy",
                        }
                    ),
                    Tone::Good,
                    mother,
                ),
                FamilyEvent::Walking { mother, child } => (
                    format!(
                        "{}, {}'s child, took their first steps",
                        name(engine, child),
                        name(engine, mother)
                    ),
                    Tone::Good,
                    child,
                ),
                FamilyEvent::Lost { mother } => (
                    format!("{}'s baby was lost with her", name(engine, mother)),
                    Tone::Bad,
                    mother,
                ),
                FamilyEvent::Arrived { woman, man } => (
                    format!(
                        "Newcomers {} and {} arrived, speaking their own way",
                        name(engine, woman),
                        name(engine, man)
                    ),
                    Tone::Talk,
                    woman,
                ),
                FamilyEvent::Conceived { .. } => continue,
            };
            let position = agent_position(engine, agent);
            if matches!(event, FamilyEvent::Walking { .. }) {
                self.push(text, tone, Some(agent), position);
            } else {
                self.tell(engine, text, tone, Some(agent), position);
            }
        }

        for event in engine.word_events() {
            if let sim_core::WordEvent::Coined {
                agent,
                form,
                concept,
            } = *event
            {
                self.push(
                    format!(
                        "{} made up a word, {}, for {}",
                        name(engine, agent),
                        labels::word(form),
                        labels::concept(concept)
                    ),
                    Tone::Talk,
                    Some(agent),
                    agent_position(engine, agent),
                );
            }
        }

        for craft in engine
            .craft_events()
            .iter()
            .filter(|craft| craft.watchers > 0)
        {
            self.push(
                format!(
                    "{} made {} while {} watched",
                    name(engine, craft.agent),
                    crate::labels::material(craft.made),
                    craft.watchers
                ),
                Tone::Good,
                Some(craft.agent),
                agent_position(engine, craft.agent),
            );
        }

        for fallen in engine
            .structure_diagnostics()
            .iter()
            .filter(|diagnostic| diagnostic.kind == sim_core::StructureDiagnosticKind::Collapsed)
        {
            self.push(
                "An empty hut fell down".to_owned(),
                Tone::Bad,
                None,
                fallen.structure.position,
            );
        }

        // The first hut and the first hearth anyone finished, and every hearth
        // after (they're rare): milestones in the story.
        for built in engine
            .structure_diagnostics()
            .iter()
            .filter(|diagnostic| diagnostic.kind == sim_core::StructureDiagnosticKind::Completed)
        {
            let hearth = built.structure.kind.burns();
            let count = &mut self.built[usize::from(hearth)];
            *count += 1;
            let first = *count == 1;
            let builder = self.builders.remove(&built.structure.id.get());
            let who = builder.map_or_else(|| "Someone".to_owned(), |agent| name(engine, agent));
            let text = match (hearth, first) {
                (false, true) => format!("{who} built the first hut"),
                (true, true) => format!("{who} built the first hearth"),
                (true, false) => format!("{who} built a hearth"),
                (false, false) => continue,
            };
            self.tell(engine, text, Tone::Good, builder, built.structure.position);
        }
        if engine.snapshot().tick % sim_core::TICKS_PER_YEAR == 0 {
            let people = engine.snapshot().living_agent_count;
            self.tell(
                engine,
                format!("Year {} begins: {people} people", year_of(engine)),
                Tone::Neutral,
                None,
                WorldPosition { x: 0, y: 0 },
            );
        }

        for fire in engine.fire_events().iter().filter(|fire| fire.relit) {
            self.push(
                format!("{} relit a fire", name(engine, fire.agent)),
                Tone::Good,
                Some(fire.agent),
                fire.at,
            );
        }

        for couple in engine.couple_events() {
            self.tell(
                engine,
                format!(
                    "{} and {} became a couple",
                    name(engine, couple.first),
                    name(engine, couple.second)
                ),
                Tone::Good,
                Some(couple.first),
                agent_position(engine, couple.first),
            );
        }

        for grief in engine.grief_events() {
            self.push(
                format!(
                    "{} mourns {}",
                    name(engine, grief.agent),
                    name(engine, grief.lost)
                ),
                Tone::Bad,
                Some(grief.agent),
                agent_position(engine, grief.agent),
            );
        }

        for event in engine.wildlife_events() {
            match *event {
                WildlifeEvent::Bite {
                    species,
                    agent,
                    position,
                    ..
                } => self.push(
                    format!("A {} bit {}", labels::species(species), name(engine, agent)),
                    Tone::Bad,
                    Some(agent),
                    position,
                ),
                WildlifeEvent::Struck {
                    species,
                    hunter,
                    helpers,
                    killed: true,
                    position,
                    ..
                } => {
                    let who = match helpers {
                        0 => name(engine, hunter),
                        1 => format!("{} and a helper", name(engine, hunter)),
                        _ => format!("{} and {helpers} helpers", name(engine, hunter)),
                    };
                    self.push(
                        format!("{who} killed a {}", labels::species(species)),
                        Tone::Good,
                        Some(hunter),
                        position,
                    );
                }
                WildlifeEvent::Killed {
                    species, position, ..
                } => self.push(
                    format!("A {} was killed by a predator", labels::species(species)),
                    Tone::Neutral,
                    None,
                    position,
                ),
                WildlifeEvent::Struck { .. } | WildlifeEvent::Born { .. } => {}
            }
        }

        for meal in engine.meal_events().iter().filter(|meal| meal.retched) {
            let position = agent_position(engine, meal.agent);
            self.push(
                format!(
                    "{} ate {} and got sick",
                    name(engine, meal.agent),
                    labels::material(meal.material)
                ),
                Tone::Bad,
                Some(meal.agent),
                position,
            );
        }

        for request in engine.request_events() {
            if let Some(material) = request.given {
                self.push(
                    format!(
                        "{} gave {} to {}",
                        name(engine, request.giver),
                        labels::material(material),
                        name(engine, request.asker)
                    ),
                    Tone::Good,
                    Some(request.asker),
                    agent_position(engine, request.asker),
                );
            }
        }

        let signals = engine.signal_events();
        for signal in signals {
            let sender = signal.signal.sender;
            let said = signal
                .signal
                .vocal
                .map(|form| format!(" {}", labels::word(form)))
                .unwrap_or_default();
            let text = match (signal.intent.effect, signal.signal.mime) {
                (DesiredEffect::Correct, _) => format!(
                    "{} corrected someone:{said} is {}",
                    name(engine, sender),
                    labels::topic(signal.intent.topic)
                ),
                (DesiredEffect::Inform, Mime::Snarl) if signal.signal.loud => {
                    format!("{} shouted a warning{said}", name(engine, sender))
                }
                (DesiredEffect::Inform, Mime::Spear) if signal.signal.loud => {
                    format!("{} called others to hunt{said}", name(engine, sender))
                }
                _ => continue,
            };
            self.push(text, Tone::Talk, Some(sender), signal.signal.origin);
        }

        for lesson in engine.lesson_events() {
            if lesson.cause == LessonCause::Correction && lesson.use_worked == Some(false) {
                self.push(
                    format!(
                        "{} realised {} was misunderstood",
                        name(engine, lesson.agent),
                        labels::word(lesson.form)
                    ),
                    Tone::Talk,
                    Some(lesson.agent),
                    agent_position(engine, lesson.agent),
                );
            }
        }

        for reading in engine.interpretation_events() {
            let Some(signal) = signals.iter().find(|signal| signal.id == reading.signal) else {
                continue;
            };
            let meant = signal.intent.topic;
            let mixed_up = signal.intent.effect == DesiredEffect::Inform
                && meant != reading.understood
                && !matches!(meant, GestureTopic::Explored)
                && !matches!(reading.understood, GestureTopic::Explored);
            if !mixed_up {
                continue;
            }
            let what = reading
                .heard
                .map_or_else(|| "a gesture".to_owned(), labels::word);
            self.push(
                format!(
                    "{} mistook {what} for {} (meant {})",
                    name(engine, reading.receiver),
                    labels::topic(reading.understood),
                    labels::topic(meant)
                ),
                Tone::Talk,
                Some(reading.receiver),
                agent_position(engine, reading.receiver),
            );
        }
    }
}

/// What everyone calls `agent` (its given name), or "Person N".
/// The year (1 for the first) the engine is in.
fn year_of(engine: &Engine) -> u32 {
    (engine.snapshot().tick / sim_core::TICKS_PER_YEAR) as u32 + 1
}

fn name(engine: &Engine, agent: AgentId) -> String {
    engine
        .life(agent)
        .map_or_else(|| labels::person(agent), |life| life.name.spoken())
}

fn agent_position(engine: &Engine, agent: AgentId) -> WorldPosition {
    engine
        .agent_views(crate::startup::VIEWER_AGENT_LIMIT)
        .find(|view| view.id == agent)
        .map_or(WorldPosition { x: 0, y: 0 }, |view| view.position)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_feed_is_bounded_and_keeps_the_newest_entries() {
        let mut feed = Feed::default();
        for index in 0..FEED_CAPACITY + 3 {
            feed.push(
                format!("entry {index}"),
                Tone::Neutral,
                None,
                WorldPosition { x: 0, y: 0 },
            );
        }
        assert_eq!(feed.entries().len(), FEED_CAPACITY);
        assert_eq!(feed.entries().next().unwrap().text, "entry 3");
        let origin = WorldPosition { x: 0, y: 0 };
        feed.push("again".to_owned(), Tone::Good, None, origin);
        feed.push("again".to_owned(), Tone::Good, None, origin);
        assert_eq!(
            feed.entries().last().unwrap().count,
            2,
            "repeats fold together"
        );
        feed.clear();
        assert_eq!(feed.entries().len(), 0);
    }
}
