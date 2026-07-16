use sim_core::{
    AgentView, Engine, EngineConfig, MovementOutcomeKind, PopulationInit, TraversalStep,
    WorldConfig, WorldPosition,
};
use std::collections::BTreeSet;

fn initialized_engine() -> Engine {
    let mut engine = Engine::new(EngineConfig {
        seed: 42,
        ticks_per_second: 60,
        world: WorldConfig::new(128, 128).unwrap(),
    });
    engine.materialize_initial_area().unwrap();
    engine
        .initialize_population(
            PopulationInit {
                active_area: engine.world().initial_bounds(),
                population: 20,
            },
            &[],
        )
        .unwrap();
    engine
}

fn schedule_canonical_steps(engine: &mut Engine, reverse: bool) -> usize {
    let bounds = engine.world().initial_bounds();
    let mut agents: Vec<AgentView> = engine.agent_views(20).collect();
    let occupied: BTreeSet<_> = agents.iter().map(|agent| agent.position).collect();
    if reverse {
        agents.reverse();
    }
    let mut scheduled = 0;
    for agent in agents {
        let target = [(1, 0), (0, 1), (-1, 0), (0, -1)]
            .into_iter()
            .map(|(dx, dy)| WorldPosition {
                x: agent.position.x + dx,
                y: agent.position.y + dy,
            })
            .find(|&target| {
                bounds.contains(target)
                    && !occupied.contains(&target)
                    && engine
                        .world()
                        .traversal_step(agent.position, target)
                        .is_ok_and(TraversalStep::is_passable)
            });
        if let Some(target) = target {
            engine.request_move(agent.id, target).unwrap();
            scheduled += 1;
        }
    }
    scheduled
}

#[test]
fn public_slice_zero_scenario_replays_and_ignores_event_insertion_order() {
    let mut forward = initialized_engine();
    let mut reverse = initialized_engine();
    let forward_scheduled = schedule_canonical_steps(&mut forward, false);
    let reverse_scheduled = schedule_canonical_steps(&mut reverse, true);
    assert_eq!(forward_scheduled, reverse_scheduled);
    assert!(forward_scheduled > 0);

    let mut forward_moved = 0;
    let mut reverse_moved = 0;
    for _ in 0..64 {
        forward.tick();
        reverse.tick();
        forward_moved += forward
            .movement_outcomes()
            .iter()
            .filter(|outcome| outcome.kind == MovementOutcomeKind::Moved)
            .count();
        reverse_moved += reverse
            .movement_outcomes()
            .iter()
            .filter(|outcome| outcome.kind == MovementOutcomeKind::Moved)
            .count();
    }

    assert_eq!(forward_moved, forward_scheduled);
    assert_eq!(reverse_moved, reverse_scheduled);
    assert_eq!(
        forward.agent_views(20).collect::<Vec<_>>(),
        reverse.agent_views(20).collect::<Vec<_>>()
    );
    assert_eq!(forward.snapshot(), reverse.snapshot());
}
