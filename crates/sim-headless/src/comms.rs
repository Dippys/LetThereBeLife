//! Communication log: every exchange as sender intent → public gesture → each
//! receiver's reading → what it did → what it found. Built only from engine
//! diagnostics after the fact; agents never see any of it.

use std::fmt;

use sim_core::{
    AgentId, DesiredEffect, Engine, GestureTopic, InterpretationEvent, LandmarkKind, LessonCause,
    LessonEvent, PhysicalGoal, PolicyDiagnosticKind, PolicyReason, RepairEvent, RepairResponse,
    RequestResponse, SignalEvent,
};

/// One receiver's side of an exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reception {
    pub interpretation: InterpretationEvent,
    /// First tick the receiver headed for a place it was told about: matched by
    /// kind for need-driven trips, or to its latest place hint when it went to
    /// check one out (the log can't tell which of several hints it chose).
    pub acted_at: Option<u64>,
    /// `(confirmed, tick)`: found what the hint promised, or searched and gave up.
    pub outcome: Option<(bool, u64)>,
    /// If the listener asked "this?": the speaker's answer.
    pub repair: Option<RepairEvent>,
}

impl Reception {
    /// When the receiver acted on what it understood: it headed for the place
    /// because of it, or went to the spot and searched it (the hint's outcome).
    pub fn acted_on(&self) -> Option<u64> {
        self.acted_at.or(self.outcome.map(|(_, at)| at))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exchange {
    pub signal: SignalEvent,
    pub receptions: Vec<Reception>,
    /// For requests: how the one asked answered.
    pub answer: Option<RequestResponse>,
}

/// Totals over the whole log.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CommunicationSummary {
    pub exchanges: u64,
    /// Exchanges pointing at a place, by kind: water, food, wood, stone, shelter.
    pub place_exchanges: [u64; sim_core::LandmarkKind::COUNT],
    /// Warnings about and calls to hunt each species (by `Species as usize`).
    pub animal_exchanges: [u64; sim_core::Species::COUNT],
    /// Receptions acted on by running from or going after a pointed-out animal.
    pub leads_followed: u64,
    pub explored_exchanges: u64,
    pub receptions: u64,
    /// Receptions that changed the receiver's beliefs.
    pub informed: u64,
    /// Informed receptions the receiver acted on.
    pub acted: u64,
    pub confirmed: u64,
    pub refuted: u64,
    /// Receptions whose reading differed from the sender's private intent.
    pub misread: u64,
    /// Receptions where a word was heard, split by the first and second half of
    /// the log: `[early, late]`.
    pub worded: [u64; 2],
    /// ...of which the listener already read the word as the sender meant it.
    pub word_agreed: [u64; 2],
    /// Misreadings by reason (a misreading can have several):
    /// ambiguous mime, unknown word, word disagrees, need bias, memory bias.
    pub misread_reasons: [u64; 5],
    /// Misreadings the receiver acted on.
    pub misread_acted: u64,
    /// "This?" questions, and how many the speaker answered with a repair.
    pub questions: u64,
    pub repaired: u64,
    /// Word lessons by cause: consequence, confirmation, repair, correction, usage.
    pub lessons: [u64; 5],
    /// Corrections made ("you said X, but it was this, not that").
    pub corrections: u64,
    /// Consequence lessons that dropped the meaning the speaker had in mind.
    pub false_lessons: u64,
    /// Complete episodes of the project's definition of success (see `success_episodes`).
    pub success_episodes: u64,
    /// See `CommunicationLog::success_funnel`.
    pub success_funnel: [u64; 5],
    /// Requests for food: asked, first misread, given, refused, nothing to give.
    pub requests: [u64; 5],
}

/// The project's definition of success, observed: a listener misread a signal
/// for a recorded reason, acted on it, learned from what it found, and the
/// speaker then learned its word was misheard, each from observable evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuccessEpisode {
    /// Index into `exchanges()`.
    pub exchange: usize,
    pub listener: AgentId,
    pub acted_at: u64,
    pub listener_lesson: LessonEvent,
    pub speaker_lesson: LessonEvent,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CommunicationLog {
    exchanges: Vec<Exchange>,
    lessons: Vec<LessonEvent>,
    requests: Vec<sim_core::RequestEvent>,
    leads_followed: u64,
}

const fn kind_for_goal(goal: PhysicalGoal) -> Option<LandmarkKind> {
    match goal {
        PhysicalGoal::SeekWater => Some(LandmarkKind::Water),
        PhysicalGoal::SeekFood => Some(LandmarkKind::BERRIES),
        PhysicalGoal::SeekShelter => Some(LandmarkKind::SHELTER),
        _ => None,
    }
}

impl CommunicationLog {
    /// Folds the latest tick's diagnostics into the log.
    pub fn record_tick(&mut self, engine: &Engine) {
        for signal in engine.signal_events() {
            self.exchanges.push(Exchange {
                signal: *signal,
                receptions: Vec::new(),
                answer: None,
            });
        }
        for interpretation in engine.interpretation_events() {
            if let Some(exchange) = self
                .exchanges
                .iter_mut()
                .rev()
                .find(|exchange| exchange.signal.id == interpretation.signal)
            {
                exchange.receptions.push(Reception {
                    interpretation: *interpretation,
                    acted_at: None,
                    outcome: None,
                    repair: None,
                });
            }
        }
        for decision in engine.policy_diagnostics() {
            if decision.kind != PolicyDiagnosticKind::Selected
                || decision.reason != PolicyReason::ToldPlace
            {
                continue;
            }
            // Checking out a hint (goal Explore) names no kind: any place hint counts.
            let kind = kind_for_goal(decision.goal);
            if let Some(reception) =
                self.latest_reception(decision.agent, None, kind, |reception| {
                    reception.interpretation.changed && reception.acted_at.is_none()
                })
            {
                reception.acted_at = Some(decision.at.ticks());
            }
        }
        for repair in engine.repair_events() {
            if let Some(reception) = self
                .exchanges
                .iter_mut()
                .rev()
                .find(|exchange| exchange.signal.id == repair.signal)
                .and_then(|exchange| {
                    exchange
                        .receptions
                        .iter_mut()
                        .find(|reception| reception.interpretation.receiver == repair.listener)
                })
            {
                reception.repair = Some(*repair);
            }
        }
        self.lessons.extend_from_slice(engine.lesson_events());
        self.requests.extend_from_slice(engine.request_events());
        for lead in engine.lead_events() {
            if let Some(reception) = self
                .exchanges
                .iter_mut()
                .rev()
                .find(|exchange| exchange.signal.id == lead.signal)
                .and_then(|exchange| {
                    exchange
                        .receptions
                        .iter_mut()
                        .find(|reception| reception.interpretation.receiver == lead.agent)
                })
                && reception.acted_at.is_none()
            {
                reception.acted_at = Some(lead.at.ticks());
                self.leads_followed += 1;
            }
        }
        for request in engine.request_events() {
            if let Some(exchange) = self
                .exchanges
                .iter_mut()
                .rev()
                .find(|exchange| exchange.signal.id == request.signal)
            {
                exchange.answer = Some(request.response);
            }
        }
        for outcome in engine.hint_outcomes() {
            if let Some(reception) = self.latest_reception(
                outcome.agent,
                outcome.teller,
                Some(outcome.kind),
                |reception| reception.outcome.is_none(),
            ) {
                reception.outcome = Some((outcome.confirmed, outcome.at.ticks()));
            }
        }
    }

    /// The most recent reception by `receiver` about `kind` (from `sender`, if given).
    fn latest_reception(
        &mut self,
        receiver: AgentId,
        sender: Option<AgentId>,
        kind: Option<LandmarkKind>,
        accept: impl Fn(&Reception) -> bool,
    ) -> Option<&mut Reception> {
        self.exchanges
            .iter_mut()
            .rev()
            .filter(|exchange| sender.is_none_or(|sender| exchange.signal.signal.sender == sender))
            .flat_map(|exchange| exchange.receptions.iter_mut())
            .find(|reception| {
                reception.interpretation.receiver == receiver
                    && match (kind, reception.interpretation.understood) {
                        (Some(kind), understood) => understood == GestureTopic::Place(kind),
                        (None, GestureTopic::Place(_)) => true,
                        (None, GestureTopic::Explored | GestureTopic::Animal(_)) => false,
                    }
                    && accept(reception)
            })
    }

    pub fn exchanges(&self) -> &[Exchange] {
        &self.exchanges
    }

    pub fn lessons(&self) -> &[LessonEvent] {
        &self.lessons
    }

    /// Every complete success episode, followed exactly through gesture ids:
    /// a listener misread a gesture (for a recorded reason), went to the place it
    /// had in mind, and learned from what it found there (relearning the word, or
    /// wrongly confirming its misreading); then the speaker changed what it
    /// believes about that word because of something that same listener visibly
    /// did (corrected it, or used it in the other sense).
    /// Index of the exchange for gesture `id`. Gesture ids only increase, so
    /// the log is sorted by them.
    fn exchange_index(&self, id: u64) -> Option<usize> {
        self.exchanges
            .binary_search_by_key(&id, |exchange| exchange.signal.id)
            .ok()
    }

    pub fn success_episodes(&self) -> Vec<SuccessEpisode> {
        self.trace_episodes().0
    }

    /// How far candidate episodes get: listener consequence lessons, of those
    /// tied to an informing gesture, with a misread reception of that word, about
    /// the misreading (and acted on first), and with a speaker lesson caused by
    /// that listener.
    pub fn success_funnel(&self) -> [u64; 5] {
        self.trace_episodes().1
    }

    fn trace_episodes(&self) -> (Vec<SuccessEpisode>, [u64; 5]) {
        let mut funnel = [0_u64; 5];
        let exchange_by_id = |id: u64| self.exchange_index(id);
        let mut episodes = Vec::new();
        for listener_lesson in &self.lessons {
            let (LessonCause::Consequence, Some(id)) =
                (listener_lesson.cause, listener_lesson.signal)
            else {
                continue;
            };
            funnel[0] += 1;
            let Some(index) = exchange_by_id(id) else {
                continue;
            };
            let exchange = &self.exchanges[index];
            let meant = exchange.signal.intent.topic.concept();
            if exchange.signal.intent.effect != DesiredEffect::Inform {
                continue;
            }
            funnel[1] += 1;
            let Some(reception) = exchange.receptions.iter().find(|reception| {
                reception.interpretation.receiver == listener_lesson.agent
                    && reception.interpretation.heard == Some(listener_lesson.form)
                    && reception.interpretation.understood.concept() != meant
            }) else {
                continue;
            };
            funnel[2] += 1;
            let misread = reception.interpretation.understood.concept();
            // The lesson must be about the misreading: dropping it or confirming it.
            if listener_lesson.weakened != Some(misread)
                && listener_lesson.strengthened != Some(misread)
            {
                continue;
            }
            funnel[3] += 1;
            // It must have acted on its reading before it learned better.
            let Some(acted_at) = reception
                .acted_on()
                .filter(|&acted| acted <= listener_lesson.at.ticks())
            else {
                continue;
            };
            let speaker = exchange.signal.signal.sender;
            let Some(speaker_lesson) = self.lessons.iter().find(|lesson| {
                let by_listener = lesson.signal.and_then(exchange_by_id).is_some_and(|other| {
                    self.exchanges[other].signal.signal.sender == listener_lesson.agent
                });
                // About the meaning at stake: it now doubts what it meant, or
                // takes up the meaning the listener gave the word.
                let about_it =
                    lesson.weakened == Some(meant) || lesson.strengthened == Some(misread);
                lesson.agent == speaker
                    && lesson.form == listener_lesson.form
                    && lesson.at >= listener_lesson.at
                    && by_listener
                    && about_it
                    && match lesson.cause {
                        LessonCause::Correction => lesson.use_worked == Some(false),
                        // Heard the listener use the word, or went where the
                        // listener pointed with it and saw what was there.
                        LessonCause::Usage | LessonCause::Consequence => true,
                        LessonCause::Confirmation | LessonCause::Repair => false,
                    }
            }) else {
                continue;
            };
            funnel[4] += 1;
            episodes.push(SuccessEpisode {
                exchange: index,
                listener: listener_lesson.agent,
                acted_at,
                listener_lesson: *listener_lesson,
                speaker_lesson: *speaker_lesson,
            });
        }
        (episodes, funnel)
    }

    /// A step-by-step account of one success episode.
    pub fn describe_episode(&self, episode: SuccessEpisode) -> String {
        let exchange = &self.exchanges[episode.exchange];
        let signal = exchange.signal;
        let reception = exchange
            .receptions
            .iter()
            .find(|reception| reception.interpretation.receiver == episode.listener)
            .expect("episode refers to a reception");
        let read = reception.interpretation;
        let form = read
            .heard
            .map_or_else(|| "-".to_owned(), |form| form.name());
        let reasons: Vec<&str> = [
            (
                read.reading.reasons.ambiguous_mime,
                "the mime looked ambiguous",
            ),
            (
                read.reading.reasons.unknown_word,
                "the word was unknown to it",
            ),
            (
                read.reading.reasons.word_disagrees,
                "the word meant something else to it",
            ),
            (read.reading.reasons.need_bias, "its own need"),
            (
                read.reading.reasons.memory_bias,
                "what it remembered near there",
            ),
        ]
        .into_iter()
        .filter_map(|(applies, why)| applies.then_some(why))
        .collect();
        let name = |concept: Option<sim_core::Concept>| {
            concept.map_or_else(
                || "?".to_owned(),
                |concept| format!("{concept:?}").to_uppercase(),
            )
        };
        let listener_step = if episode.listener_lesson.weakened.is_some() {
            format!(
                "it found {} there instead, and now takes \"{form}\" to mean that rather than {}.",
                name(episode.listener_lesson.strengthened),
                name(episode.listener_lesson.weakened)
            )
        } else {
            format!(
                "it happened to find {} there too, which convinced it \"{form}\" means {}.",
                name(episode.listener_lesson.strengthened),
                name(episode.listener_lesson.strengthened)
            )
        };
        let speaker_step = match episode.speaker_lesson.cause {
            LessonCause::Correction => format!(
                "saw agent {} correct \"{form}\" and now doubts it means {}, since it was taken otherwise.",
                episode.listener.get(),
                name(episode.speaker_lesson.weakened)
            ),
            LessonCause::Consequence => format!(
                "went where agent {} pointed with \"{form}\", found {} there, and now doubts it means {}.",
                episode.listener.get(),
                name(episode.speaker_lesson.strengthened),
                name(episode.speaker_lesson.weakened)
            ),
            _ => format!(
                "heard agent {} use \"{form}\" for {} and now doubts it means {}.",
                episode.listener.get(),
                name(episode.speaker_lesson.strengthened),
                name(episode.speaker_lesson.weakened)
            ),
        };
        format!(
            "1. t={} agent {} pointed, mimed {:?} and said \"{form}\", privately meaning {}.\n\
             2. Agent {} read it as {} because {}.\n\
             3. At t={} it acted on that reading at the place.\n\
             4. At t={} {listener_step}\n\
             5. At t={} agent {} {speaker_step}",
            signal.at.ticks(),
            signal.signal.sender.get(),
            signal.signal.mime,
            topic_name(signal.intent.topic),
            read.receiver.get(),
            topic_name(read.understood),
            if reasons.is_empty() {
                "of the evidence it weighed".to_owned()
            } else {
                reasons.join(" and ")
            },
            episode.acted_at,
            episode.listener_lesson.at.ticks(),
            episode.speaker_lesson.at.ticks(),
            signal.signal.sender.get(),
        )
    }

    /// Exchanges where `agent` was the sender or a receiver.
    pub fn involving(&self, agent: AgentId) -> impl Iterator<Item = &Exchange> + '_ {
        self.exchanges.iter().filter(move |exchange| {
            exchange.signal.signal.sender == agent
                || exchange
                    .receptions
                    .iter()
                    .any(|reception| reception.interpretation.receiver == agent)
        })
    }

    pub fn summary(&self) -> CommunicationSummary {
        let mut summary = CommunicationSummary::default();
        for lesson in &self.lessons {
            let slot = match lesson.cause {
                LessonCause::Consequence => 0,
                LessonCause::Confirmation => 1,
                LessonCause::Repair => 2,
                LessonCause::Correction => 3,
                LessonCause::Usage => 4,
            };
            summary.lessons[slot] += 1;
            // A consequence lesson that drops what the speaker meant is a false lesson.
            if lesson.cause == LessonCause::Consequence
                && let Some(exchange) = lesson
                    .signal
                    .and_then(|id| self.exchange_index(id))
                    .map(|index| &self.exchanges[index])
                && lesson.weakened == Some(exchange.signal.intent.topic.concept())
            {
                summary.false_lessons += 1;
            }
        }
        let (episodes, funnel) = self.trace_episodes();
        summary.success_episodes = episodes.len() as u64;
        summary.success_funnel = funnel;
        summary.leads_followed = self.leads_followed;
        for request in &self.requests {
            summary.requests[0] += 1;
            summary.requests[1] += u64::from(request.read_as != sim_core::Concept::BERRIES);
            summary.requests[match request.response {
                RequestResponse::Gave => 2,
                RequestResponse::Refused => 3,
                RequestResponse::NothingToGive => 4,
            }] += 1;
        }
        let half = self.exchanges.len() / 2;
        for (index, exchange) in self.exchanges.iter().enumerate() {
            let period = usize::from(index >= half);
            for reception in &exchange.receptions {
                if reception.interpretation.heard.is_some() {
                    summary.worded[period] += 1;
                    summary.word_agreed[period] += u64::from(
                        reception.interpretation.word_reading
                            == Some(exchange.signal.intent.topic.concept()),
                    );
                }
            }
            summary.exchanges += 1;
            summary.corrections +=
                u64::from(exchange.signal.intent.effect == DesiredEffect::Correct);
            match exchange.signal.intent.topic {
                _ if exchange.signal.intent.effect == DesiredEffect::Request => {}
                GestureTopic::Place(kind) => summary.place_exchanges[kind.index()] += 1,
                GestureTopic::Explored => summary.explored_exchanges += 1,
                GestureTopic::Animal(species) => summary.animal_exchanges[species as usize] += 1,
            }
            for reception in &exchange.receptions {
                summary.receptions += 1;
                summary.informed += u64::from(reception.interpretation.changed);
                if let Some(repair) = reception.repair {
                    summary.questions += 1;
                    summary.repaired +=
                        u64::from(matches!(repair.response, RepairResponse::Repaired(_)));
                }
                summary.acted += u64::from(reception.acted_on().is_some());
                if reception.interpretation.understood != exchange.signal.intent.topic {
                    summary.misread += 1;
                    summary.misread_acted += u64::from(reception.acted_on().is_some());
                    let reasons = reception.interpretation.reading.reasons;
                    for (slot, applies) in [
                        reasons.ambiguous_mime,
                        reasons.unknown_word,
                        reasons.word_disagrees,
                        reasons.need_bias,
                        reasons.memory_bias,
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        summary.misread_reasons[slot] += u64::from(applies);
                    }
                }
                match reception.outcome {
                    Some((true, _)) => summary.confirmed += 1,
                    Some((false, _)) => summary.refuted += 1,
                    None => {}
                }
            }
        }
        summary
    }
}

impl fmt::Display for CommunicationSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let places: Vec<String> = sim_core::LandmarkKind::ALL
            .into_iter()
            .map(|kind| {
                let name = format!("{:?}", kind.concept())
                    .to_lowercase()
                    .replace("material(", "")
                    .replace("structure(", "")
                    .replace(')', "");
                format!("{name} {}", self.place_exchanges[kind.index()])
            })
            .collect();
        write!(
            formatter,
            "  communication: exchanges={} ({}, explored {}) receptions={} informed={} acted={} confirmed={} refuted={} misread={}",
            self.exchanges,
            places.join(", "),
            self.explored_exchanges,
            self.receptions,
            self.informed,
            self.acted,
            self.confirmed,
            self.refuted,
            self.misread
        )?;
        let percent = |part: u64, whole: u64| (part * 100).checked_div(whole).unwrap_or(0);
        write!(
            formatter,
            "\n  words: heard {} / {} (early / late half); listener already read the word as meant {}% -> {}%",
            self.worded[0],
            self.worded[1],
            percent(self.word_agreed[0], self.worded[0]),
            percent(self.word_agreed[1], self.worded[1])
        )?;
        let [ambiguous, unknown, disagrees, need, memory] = self.misread_reasons;
        write!(
            formatter,
            "\n  misreadings: {} of {} receptions ({} acted on); reasons: ambiguous mime {ambiguous}, unknown word {unknown}, word disagrees {disagrees}, need bias {need}, memory bias {memory}",
            self.misread, self.receptions, self.misread_acted
        )?;
        let [consequence, confirmation, repair, correction, usage] = self.lessons;
        write!(
            formatter,
            "\n  repair: questions {} (repaired {}), corrections {}; word lessons: consequence {consequence} ({} against what was meant), confirmation {confirmation}, repair {repair}, correction {correction}, usage {usage}; SUCCESS EPISODES {}",
            self.questions,
            self.repaired,
            self.corrections,
            self.false_lessons,
            self.success_episodes
        )?;
        let [lessons, informing, misread, about, speaker] = self.success_funnel;
        write!(
            formatter,
            "\n  episode funnel: consequence lessons {lessons} -> from a gesture {informing} -> misread {misread} -> about the misreading {about} -> speaker learned {speaker}"
        )?;
        let [deer, wolves] = self.animal_exchanges;
        write!(
            formatter,
            "\n  animals: calls to hunt deer {deer}, wolf warnings {wolves}; acted on {}",
            self.leads_followed
        )?;
        let [asked, misread, gave, refused, empty] = self.requests;
        write!(
            formatter,
            "\n  requests for food: {asked} (first misread {misread}); gave {gave}, refused {refused}, nothing to give {empty}"
        )
    }
}

fn topic_name(topic: GestureTopic) -> String {
    match topic {
        GestureTopic::Place(kind) => format!("{kind:?}").to_uppercase(),
        GestureTopic::Explored => "BEEN-THERE".to_owned(),
        GestureTopic::Animal(species) => format!("{species:?}").to_uppercase(),
    }
}

impl fmt::Display for Exchange {
    /// A short narrative of one exchange, private intent marked as such.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let event = self.signal;
        let public = event.signal;
        if let Some(giver) = public.addressee {
            write!(
                formatter,
                "#{} t={} agent {} holds out a hand to agent {}, mimes {:?}, says \"{}\", urgency {} [privately asking for {}]",
                event.id,
                event.at.ticks(),
                public.sender.get(),
                giver.get(),
                public.mime,
                public
                    .vocal
                    .map_or_else(|| "-".to_owned(), |form| form.name()),
                public.tone.urgency,
                topic_name(event.intent.topic),
            )?;
            return self.write_receptions(formatter);
        }
        let (dx, dy) = public.pointing.direction();
        write!(
            formatter,
            "#{} t={} agent {} at ({},{}) points dir ({dx},{dy}) emphasis {}, mimes {:?}, says \"{}\", urgency {} [privately meant {} at ({},{})] -> watchers infer ({},{}) +/-{}",
            event.id,
            event.at.ticks(),
            public.sender.get(),
            public.origin.x,
            public.origin.y,
            public.pointing.emphasis(),
            public.mime,
            public
                .vocal
                .map_or_else(|| "-".to_owned(), |form| form.name()),
            public.tone.urgency,
            topic_name(event.intent.topic),
            event.intent.place.x,
            event.intent.place.y,
            event.inferred_position.x,
            event.inferred_position.y,
            event.search_radius
        )?;
        self.write_receptions(formatter)
    }
}

impl Exchange {
    fn write_receptions(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let event = self.signal;
        for reception in &self.receptions {
            let read = reception.interpretation;
            let reading = read.reading;
            let candidates: Vec<String> = reading.candidates
                [..usize::from(reading.candidate_count)]
                .iter()
                .map(|(concept, probability)| {
                    format!("{concept:?} {}%", u32::from(*probability) * 100 / 255).to_uppercase()
                })
                .collect();
            let reasons: Vec<&str> = [
                (reading.reasons.ambiguous_mime, "ambiguous mime"),
                (reading.reasons.unknown_word, "unknown word"),
                (reading.reasons.word_disagrees, "word disagrees"),
                (reading.reasons.need_bias, "own need"),
                (reading.reasons.memory_bias, "own memory"),
            ]
            .into_iter()
            .filter_map(|(applies, name)| applies.then_some(name))
            .collect();
            let misread = read.understood != event.intent.topic;
            write!(
                formatter,
                "\n      agent {} read {}{}{} [{}] {} conf {}",
                read.receiver.get(),
                topic_name(read.understood),
                if misread { " (MISREAD)" } else { "" },
                if read.changed || self.answer.is_some() {
                    ""
                } else {
                    " (already knew)"
                },
                candidates.join(" / "),
                if reasons.is_empty() {
                    String::new()
                } else {
                    format!("because {}", reasons.join(", "))
                },
                read.confidence
            )?;
            if let Some(form) = read.heard {
                let meaning = read.word_reading.map_or_else(
                    || "nothing yet".to_owned(),
                    |concept| format!("{concept:?}"),
                );
                write!(formatter, "; took \"{}\" to mean {meaning}", form.name())?;
            }
            if let Some(at) = reception.acted_at {
                write!(formatter, "; went looking at t={at}")?;
            }
            match reception.outcome {
                Some((true, at)) => write!(formatter, "; found it at t={at}")?,
                Some((false, at)) => write!(formatter, "; gave up at t={at}")?,
                None => {}
            }
            match self.answer {
                Some(RequestResponse::Gave) => write!(formatter, "; handed over food")?,
                Some(RequestResponse::Refused) => write!(formatter, "; shook its head")?,
                Some(RequestResponse::NothingToGive) => write!(formatter, "; showed empty hands")?,
                None => {}
            }
        }
        Ok(())
    }
}
