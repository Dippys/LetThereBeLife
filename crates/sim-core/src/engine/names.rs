//! Names: calling someone out loud, and bystanders learning what people are called.

use crate::cognition::belief_seconds;
use crate::wildlife::mix;
use crate::{AgentId, Engine, WorldPosition, WorldRect};

/// A name called out carries this far (Chebyshev cells).
const CALL_RADIUS: i64 = 12;
/// A greeting is called to someone not seen for at least this long (seconds).
pub(super) const GREETING_GAP_SECONDS: u32 = 10 * 60;
/// One in this many bystanders takes a called name for someone standing right
/// next to the person called.
const MISHEARD_ODDS: u64 = 4;

/// Seconds between one person's grumbles.
pub(super) const GRUMBLE_GAP_SECONDS: u32 = 60 * 60;
/// Trust a listener loses in someone grumbled about by a person it trusts.
const HEARSAY_TRUST: u8 = 16;
/// Anger family take up on hearing one of their own grumble about someone.
const HEARSAY_ANGER: u8 = 16;
/// Anger at hearing oneself grumbled about.
const OVERHEARD_ANGER: u8 = 24;

/// Someone called a person by name, and how one bystander took it (latest
/// tick, for logs and tools).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NameEvent {
    pub caller: AgentId,
    pub called: AgentId,
    pub listener: AgentId,
    /// Who the listener now thinks has that name.
    pub heard_as: AgentId,
    pub name: crate::Name,
}

impl Engine {
    /// `caller` calls `called` by the name it knows them by; everyone awake
    /// within earshot who doesn't yet know a name for that person learns it,
    /// though one standing near someone else may pin it on the wrong person.
    pub(super) fn call_out(&mut self, caller: AgentId, called: AgentId) {
        let Some(name) = self.minds.get(caller).and_then(|mind| {
            mind.social
                .slot_of(called)
                .and_then(|slot| mind.social.name(slot))
        }) else {
            return;
        };
        let (Some(from), Some(target)) = (
            self.population.view(caller).map(|view| view.position),
            self.population.view(called).map(|view| view.position),
        ) else {
            return;
        };
        let mut nearby = Vec::new();
        self.population.spatial().agents_in(
            WorldRect {
                min: WorldPosition {
                    x: from.x - CALL_RADIUS,
                    y: from.y - CALL_RADIUS,
                },
                max: WorldPosition {
                    x: from.x + CALL_RADIUS + 1,
                    y: from.y + CALL_RADIUS + 1,
                },
            },
            &mut nearby,
        );
        let now = belief_seconds(self.time);
        for listener in nearby {
            if listener == caller || listener == called {
                continue;
            }
            let awake = self
                .population
                .view(listener)
                .is_some_and(|view| super::cognition::can_watch(view.activity));
            if !awake {
                continue;
            }
            let roll = mix(self.config.life_seed
                ^ 0x4e41_4d45
                ^ u64::from(listener.get()) << 32
                ^ self.time.ticks());
            let bystander = self
                .population
                .views(usize::MAX)
                .find(|view| {
                    view.id != listener
                        && view.id != caller
                        && view.id != called
                        && super::cognition::can_watch(view.activity)
                        && view
                            .position
                            .x
                            .abs_diff(target.x)
                            .max(view.position.y.abs_diff(target.y))
                            <= 1
                })
                .filter(|_| roll % MISHEARD_ODDS == 0);
            let (heard_as, at) =
                bystander.map_or((called, target), |view| (view.id, view.position));
            let mind = self.minds.get_mut(listener);
            if let Some(slot) = mind.notice(heard_as, at, now) {
                mind.social.learn_name(slot, name);
            }
            self.name_events.push(NameEvent {
                caller,
                called,
                listener,
                heard_as,
                name,
            });
        }
    }

    /// `teller` scowls and says the name it calls `about` by: those within
    /// earshot who trust it think less of whoever they call by that name,
    /// family take up the grudge, and whoever goes by that name is angered.
    /// Nobody hears who was meant, only the name.
    pub(super) fn grumble(&mut self, teller: AgentId, about: AgentId, name: crate::Name) {
        let Some(from) = self.population.view(teller).map(|view| view.position) else {
            return;
        };
        let mut nearby = Vec::new();
        self.population.spatial().agents_in(
            WorldRect {
                min: WorldPosition {
                    x: from.x - CALL_RADIUS,
                    y: from.y - CALL_RADIUS,
                },
                max: WorldPosition {
                    x: from.x + CALL_RADIUS + 1,
                    y: from.y + CALL_RADIUS + 1,
                },
            },
            &mut nearby,
        );
        nearby.sort_unstable();
        let now = belief_seconds(self.time);
        let (mut heard, mut wrong, mut overheard) = (0u8, 0u8, false);
        for listener in nearby {
            let awake = self
                .population
                .view(listener)
                .is_some_and(|view| super::cognition::can_watch(view.activity));
            if listener == teller || !awake {
                continue;
            }
            // Its own name, said with a scowl: it takes it badly.
            if self.life_of(listener).name == name {
                overheard |= listener == about;
                let mind = self.minds.get_mut(listener);
                if let Some(slot) = mind.notice(teller, from, now) {
                    mind.social.provoke(slot, OVERHEARD_ANGER);
                }
                continue;
            }
            let mind = self.minds.get_mut(listener);
            let Some(named) = mind.social.slot_named(name) else {
                continue;
            };
            let Some(source) = mind.social.slot_of(teller) else {
                continue;
            };
            if mind.social.trust(source) < crate::cognition::DEFAULT_TRUST {
                continue;
            }
            mind.social.heard_against(named, HEARSAY_TRUST);
            if mind.social.tie(source).is_some() {
                mind.social.provoke(named, HEARSAY_ANGER);
            }
            heard = heard.saturating_add(1);
            wrong = wrong.saturating_add(u8::from(mind.social.agent_in(named) != Some(about)));
        }
        self.life_events.push(crate::LifeEvent::Grumbled {
            agent: teller,
            about,
            heard,
            wrong,
            overheard,
        });
    }

    /// Names called during the latest tick (for logs and tools).
    pub fn name_events(&self) -> &[NameEvent] {
        &self.name_events
    }

    /// What `agent` calls `other`, if it knows.
    pub fn name_known(&self, agent: AgentId, other: AgentId) -> Option<crate::Name> {
        let social = &self.minds.get(agent)?.social;
        social.name(social.slot_of(other)?)
    }
}
