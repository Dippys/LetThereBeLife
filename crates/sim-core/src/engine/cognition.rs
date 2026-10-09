//! Engine side of cognition: updating mental maps from perception, memory-driven
//! deliberation, pointing gestures and who sees them, and read-only belief views.

use super::errors::{move_failure, perception_failure};
use crate::cognition::{belief_seconds, interpret, point};
use crate::policy::{MindInput, PolicyAction, PolicySelection, deliberate};
use crate::{
    AgentActivity, AgentId, Engine, ExplorationHeading, InventoryView, LandmarkKind, MentalMapView,
    PHYSICAL_POLICY_RADIUS, PhysicalGoal, PhysicalNeedsView, PhysicalPerception, PolicyDiagnostic,
    PolicyDiagnosticKind, PolicyFailureReason, PolicyOptions, PolicyReason, SHARE_COOLDOWN_SECONDS,
    SIGNAL_TICKS, SignalEvent, WorldPosition,
};

const fn can_watch(activity: AgentActivity) -> bool {
    !matches!(
        activity,
        AgentActivity::Sleeping | AgentActivity::Incapacitated | AgentActivity::Dead
    )
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
        let now = belief_seconds(self.time);
        let heading = self
            .population
            .exploration_heading(agent)
            .unwrap_or(ExplorationHeading::North);
        let map = self.minds.get_mut(agent);
        map.observe(agent.get(), origin, perception, now);
        let search_target = if map.seen_count(LandmarkKind::Water) == 0 {
            Some(map.search_target(origin, perception.area))
        } else {
            map.end_search();
            None
        };
        let someone_watching = perception
            .agents
            .iter()
            .any(|other| other.id != agent && can_watch(other.activity));
        let share = (self.policy_options.sharing
            && someone_watching
            && map.share_ready(now, SHARE_COOLDOWN_SECONDS))
        .then(|| map.shareable(perception.area))
        .flatten();
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
            },
        );
        if deliberation.selection.goal == PhysicalGoal::Signal
            && let Some((_, rank)) = share
        {
            self.minds.get_mut(agent).mark_shared(now, rank);
        }
        (deliberation.selection, deliberation.heading)
    }

    /// Begins a pointing gesture toward a remembered place.
    pub(super) fn start_signal(
        &mut self,
        agent: AgentId,
        place: WorldPosition,
        reason: PolicyReason,
    ) {
        self.population.clear_route(agent);
        match self.population.schedule_policy_action(
            &mut self.scheduler,
            self.time,
            agent,
            PolicyAction {
                goal: PhysicalGoal::Signal,
                target: place,
                reason,
                duration: SIGNAL_TICKS,
            },
        ) {
            Ok(_) => self.policy_diagnostics.push(PolicyDiagnostic {
                agent,
                at: self.time,
                goal: PhysicalGoal::Signal,
                target: Some(place),
                reason,
                kind: PolicyDiagnosticKind::ActionStarted,
                failure: None,
            }),
            Err(error) => self.schedule_policy_retry(
                agent,
                PhysicalGoal::Signal,
                Some(place),
                reason,
                move_failure(error),
            ),
        }
    }

    /// Completes a gesture: every awake agent in view of the sender sees it and
    /// infers a rough place. Watchers never learn the sender's exact memory.
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
        let kind = self
            .minds
            .get(sender)
            .and_then(|map| map.seen_kind_at(place))
            .ok_or(PolicyFailureReason::TargetUnavailable)?;
        let gesture = point(from, place).ok_or(PolicyFailureReason::TargetUnavailable)?;
        let (estimate, uncertainty) = interpret(from, gesture);
        let perception = self
            .perceive_physical(sender, PHYSICAL_POLICY_RADIUS)
            .map_err(perception_failure)?;
        let now = belief_seconds(self.time);
        let (mut watchers, mut informed) = (0_u16, 0_u16);
        for watcher in &perception.agents {
            if watcher.id == sender || !can_watch(watcher.activity) {
                continue;
            }
            watchers = watchers.saturating_add(1);
            if self
                .minds
                .get_mut(watcher.id)
                .remember_told(kind, estimate, uncertainty, now)
            {
                informed = informed.saturating_add(1);
            }
        }
        self.signal_events.push(SignalEvent {
            sender,
            at: self.time,
            kind,
            inferred_position: estimate,
            informed,
            watchers,
        });
        Ok(())
    }

    /// The cognitive features the active policy uses.
    pub fn policy_options(&self) -> PolicyOptions {
        self.policy_options
    }

    /// A copy of what `agent` believes about places. `None` before it has a mind.
    pub fn mental_map(&self, agent: AgentId) -> Option<MentalMapView> {
        let map = self.minds.get(agent)?;
        Some(MentalMapView {
            agent,
            landmarks: map.views().collect(),
            explored_tiles: map.explored_tile_count(),
        })
    }

    /// Pointing gestures completed during the latest tick.
    pub fn signal_events(&self) -> &[SignalEvent] {
        &self.signal_events
    }
}
