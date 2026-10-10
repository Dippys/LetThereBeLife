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
            let roll = mix(self.config.seed
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
