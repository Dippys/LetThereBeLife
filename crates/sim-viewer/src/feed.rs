//! Presentation-only feed of notable happenings (deaths, bites, kills, help,
//! warnings, misunderstandings), copied from the engine's per-tick logs.

use std::collections::VecDeque;

use sim_core::{AgentId, DesiredEffect, Engine, GestureTopic, Mime, WildlifeEvent, WorldPosition};

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

#[derive(Debug, Default)]
pub struct Feed {
    entries: VecDeque<FeedEntry>,
    deaths_seen: usize,
}

impl Feed {
    /// Oldest first.
    pub fn entries(&self) -> impl ExactSizeIterator<Item = &FeedEntry> {
        self.entries.iter()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.deaths_seen = 0;
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
        for death in &deaths[self.deaths_seen..] {
            self.push(
                format!(
                    "{} {}",
                    labels::person(death.agent),
                    labels::death(death.cause)
                ),
                Tone::Bad,
                Some(death.agent),
                death.position,
            );
        }
        self.deaths_seen = deaths.len();

        for event in engine.wildlife_events() {
            match *event {
                WildlifeEvent::Bite {
                    species,
                    agent,
                    position,
                    ..
                } => self.push(
                    format!(
                        "A {} bit {}",
                        labels::species(species),
                        labels::person(agent)
                    ),
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
                        0 => labels::person(hunter),
                        1 => format!("{} and a helper", labels::person(hunter)),
                        _ => format!("{} and {helpers} helpers", labels::person(hunter)),
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
                    labels::person(meal.agent),
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
                        labels::person(request.giver),
                        labels::material(material),
                        labels::person(request.asker)
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
                    "{} corrected someone about {}",
                    labels::person(sender),
                    labels::topic(signal.intent.topic)
                ),
                (DesiredEffect::Inform, Mime::Snarl) if signal.signal.loud => {
                    format!("{} shouted a warning{said}", labels::person(sender))
                }
                (DesiredEffect::Inform, Mime::Spear) if signal.signal.loud => {
                    format!("{} called others to hunt{said}", labels::person(sender))
                }
                _ => continue,
            };
            self.push(text, Tone::Talk, Some(sender), signal.signal.origin);
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
                .map_or_else(|| "the gesture".to_owned(), labels::word);
            self.push(
                format!(
                    "{} took {what} to mean {}, but {} meant {}",
                    labels::person(reading.receiver),
                    labels::topic(reading.understood),
                    labels::person(signal.signal.sender),
                    labels::topic(meant)
                ),
                Tone::Talk,
                Some(reading.receiver),
                agent_position(engine, reading.receiver),
            );
        }
    }
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
