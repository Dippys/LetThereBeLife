//! Communication log: every exchange as sender intent → public gesture → each
//! receiver's reading → what it did → what it found. Built only from engine
//! diagnostics after the fact; agents never see any of it.

use std::fmt;

use sim_core::{
    AgentId, Engine, GestureTopic, InterpretationEvent, LandmarkKind, PhysicalGoal,
    PolicyDiagnosticKind, PolicyReason, SignalEvent,
};

/// One receiver's side of an exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reception {
    pub interpretation: InterpretationEvent,
    /// First tick the receiver headed for a place it was told about, matching
    /// this exchange's kind (the log can't tell which of several hints it chose).
    pub acted_at: Option<u64>,
    /// `(confirmed, tick)`: found what the hint promised, or searched and gave up.
    pub outcome: Option<(bool, u64)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exchange {
    pub signal: SignalEvent,
    pub receptions: Vec<Reception>,
}

/// Totals over the whole log.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CommunicationSummary {
    pub exchanges: u64,
    /// Exchanges pointing at a place, by kind: water, food, wood, stone, shelter.
    pub place_exchanges: [u64; 5],
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
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CommunicationLog {
    exchanges: Vec<Exchange>,
}

const fn kind_for_goal(goal: PhysicalGoal) -> Option<LandmarkKind> {
    match goal {
        PhysicalGoal::SeekWater => Some(LandmarkKind::Water),
        PhysicalGoal::SeekFood => Some(LandmarkKind::Food),
        PhysicalGoal::SeekShelter => Some(LandmarkKind::Shelter),
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
                });
            }
        }
        for decision in engine.policy_diagnostics() {
            if decision.kind != PolicyDiagnosticKind::Selected
                || decision.reason != PolicyReason::ToldPlace
            {
                continue;
            }
            let Some(kind) = kind_for_goal(decision.goal) else {
                continue;
            };
            if let Some(reception) =
                self.latest_reception(decision.agent, None, kind, |reception| {
                    reception.interpretation.changed && reception.acted_at.is_none()
                })
            {
                reception.acted_at = Some(decision.at.ticks());
            }
        }
        for outcome in engine.hint_outcomes() {
            if let Some(reception) =
                self.latest_reception(outcome.agent, outcome.teller, outcome.kind, |reception| {
                    reception.outcome.is_none()
                })
            {
                reception.outcome = Some((outcome.confirmed, outcome.at.ticks()));
            }
        }
    }

    /// The most recent reception by `receiver` about `kind` (from `sender`, if given).
    fn latest_reception(
        &mut self,
        receiver: AgentId,
        sender: Option<AgentId>,
        kind: LandmarkKind,
        accept: impl Fn(&Reception) -> bool,
    ) -> Option<&mut Reception> {
        self.exchanges
            .iter_mut()
            .rev()
            .filter(|exchange| sender.is_none_or(|sender| exchange.signal.signal.sender == sender))
            .flat_map(|exchange| exchange.receptions.iter_mut())
            .find(|reception| {
                reception.interpretation.receiver == receiver
                    && reception.interpretation.understood == GestureTopic::Place(kind)
                    && accept(reception)
            })
    }

    pub fn exchanges(&self) -> &[Exchange] {
        &self.exchanges
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
            match exchange.signal.intent.topic {
                GestureTopic::Place(kind) => summary.place_exchanges[kind as usize] += 1,
                GestureTopic::Explored => summary.explored_exchanges += 1,
            }
            for reception in &exchange.receptions {
                summary.receptions += 1;
                summary.informed += u64::from(reception.interpretation.changed);
                summary.acted += u64::from(reception.acted_at.is_some());
                summary.misread +=
                    u64::from(reception.interpretation.understood != exchange.signal.intent.topic);
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
        let [water, food, wood, stone, shelter] = self.place_exchanges;
        write!(
            formatter,
            "  communication: exchanges={} (water {water}, food {food}, wood {wood}, stone {stone}, shelter {shelter}, explored {}) receptions={} informed={} acted={} confirmed={} refuted={} misread={}",
            self.exchanges,
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
        )
    }
}

fn topic_name(topic: GestureTopic) -> String {
    match topic {
        GestureTopic::Place(kind) => format!("{kind:?}").to_uppercase(),
        GestureTopic::Explored => "BEEN-THERE".to_owned(),
    }
}

impl fmt::Display for Exchange {
    /// A short narrative of one exchange, private intent marked as such.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let event = self.signal;
        let public = event.signal;
        let (dx, dy) = public.pointing.direction();
        write!(
            formatter,
            "t={} agent {} at ({},{}) points dir ({dx},{dy}) emphasis {}, mimes {:?}, says \"{}\", urgency {} [privately meant {} at ({},{})] -> watchers infer ({},{}) +/-{}",
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
        for reception in &self.receptions {
            let read = reception.interpretation;
            write!(
                formatter,
                "\n      agent {} read {}{} conf {}",
                read.receiver.get(),
                topic_name(read.understood),
                if read.changed { "" } else { " (already knew)" },
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
        }
        Ok(())
    }
}
