use sim_core::{
    AgentId, Engine, EngineCommand, EngineConfig, NeedKind, NeedThresholdOutcomeKind,
    PopulationInit, RouteRequest, Standability, TraversalStep, WorldConfig, WorldPosition,
};

fn resident_engine() -> Engine {
    let mut engine = Engine::new(EngineConfig {
        seed: 42,
        ticks_per_second: 60,
        world: WorldConfig::new(128, 128).unwrap(),
    });
    engine.materialize_initial_area().unwrap();
    engine
}

fn initialized_engine() -> Engine {
    let mut engine = resident_engine();
    engine
        .initialize_population(
            PopulationInit {
                active_area: engine.world().initial_bounds(),
                population: 1,
            },
            &[],
        )
        .unwrap();
    engine
}

fn standable_step(engine: &Engine) -> (WorldPosition, WorldPosition, u16) {
    let bounds = engine.world().initial_bounds();
    for y in bounds.min.y..bounds.max.y {
        for x in bounds.min.x..bounds.max.x {
            let from = WorldPosition { x, y };
            if engine.world().standability_at(from) != Ok(Standability::Standable) {
                continue;
            }
            for (dx, dy) in [(1, 0), (0, 1), (-1, 0), (0, -1)] {
                let to = WorldPosition {
                    x: x + dx,
                    y: y + dy,
                };
                if let Ok(step) = engine.world().traversal_step(from, to)
                    && let Some(cost) = step.cost()
                {
                    return (from, to, cost);
                }
            }
        }
    }
    panic!("seeded world should contain a standable step");
}

fn passable_corridor(engine: &Engine) -> [WorldPosition; 3] {
    let bounds = engine.world().initial_bounds();
    for y in bounds.min.y..bounds.max.y {
        for x in bounds.min.x..bounds.max.x - 2 {
            let cells = [
                WorldPosition { x, y },
                WorldPosition { x: x + 1, y },
                WorldPosition { x: x + 2, y },
            ];
            if engine
                .world()
                .traversal_step(cells[0], cells[1])
                .is_ok_and(TraversalStep::is_passable)
                && engine
                    .world()
                    .traversal_step(cells[1], cells[2])
                    .is_ok_and(TraversalStep::is_passable)
            {
                return cells;
            }
        }
    }
    panic!("seeded world should contain a passable three-cell corridor");
}

#[test]
fn analytical_thresholds_are_pause_safe_chunking_independent_and_replayable() {
    let mut delayed = resident_engine();
    for _ in 0..123 {
        delayed.tick();
    }
    delayed
        .initialize_population(
            PopulationInit {
                active_area: delayed.world().initial_bounds(),
                population: 1,
            },
            &[],
        )
        .unwrap();
    let delayed_initial = delayed.physical_needs(AgentId::new(0)).unwrap();
    assert_eq!(delayed_initial.thirst.value, 0);
    assert_eq!(delayed_initial.next_threshold.unwrap().due.ticks(), 90_123);

    let mut direct = initialized_engine();
    let mut observed = initialized_engine();
    let initial = direct.physical_needs(AgentId::new(0)).unwrap();
    assert_eq!(initial.hunger.value, 0);
    assert_eq!(initial.thirst.value, 0);
    assert_eq!(initial.rest.value, 0);
    assert_eq!(initial.exposure.value, 0);
    assert_eq!(initial.next_threshold.unwrap().kind, NeedKind::Thirst);
    assert_eq!(initial.next_threshold.unwrap().due.ticks(), 90_000);

    direct.command(EngineCommand::SetPaused(true));
    for _ in 0..100 {
        direct.tick();
    }
    assert_eq!(direct.physical_needs(AgentId::new(0)).unwrap(), initial);
    direct.command(EngineCommand::SetPaused(false));

    let mut direct_events = Vec::new();
    let mut observed_events = Vec::new();
    for tick in 1..=90_000 {
        direct.tick();
        observed.tick();
        direct_events.extend_from_slice(direct.need_threshold_outcomes());
        observed_events.extend_from_slice(observed.need_threshold_outcomes());
        if tick % 997 == 0 {
            std::hint::black_box(observed.physical_needs(AgentId::new(0)).unwrap());
        }
    }
    assert_eq!(
        direct.physical_needs(AgentId::new(0)),
        observed.physical_needs(AgentId::new(0))
    );
    assert_eq!(direct_events, observed_events);
    assert_eq!(direct_events.len(), 1);
    assert_eq!(direct_events[0].kind, NeedKind::Thirst);
    assert_eq!(direct_events[0].outcome, NeedThresholdOutcomeKind::Reached);
    assert_eq!(direct_events[0].value, Some(6_000));
    assert_eq!(direct.snapshot(), observed.snapshot());

    direct.command(EngineCommand::Reset);
    direct
        .initialize_population(
            PopulationInit {
                active_area: direct.world().initial_bounds(),
                population: 1,
            },
            &[],
        )
        .unwrap();
    assert_eq!(direct.physical_needs(AgentId::new(0)).unwrap(), initial);
}

#[test]
fn movement_rebases_exact_rates_and_superseded_thresholds_are_harmless() {
    let base = resident_engine();
    let (from, target, cost) = standable_step(&base);
    let mut engine = resident_engine();
    engine
        .initialize_population(
            PopulationInit {
                active_area: engine.world().initial_bounds(),
                population: 1,
            },
            &[from],
        )
        .unwrap();
    engine.request_move(AgentId::new(0), target).unwrap();
    for _ in 0..60 {
        engine.tick();
    }
    let needs = engine.physical_needs(AgentId::new(0)).unwrap();
    let moving_ticks = u64::from(cost.min(60));
    let idle_ticks = 60 - moving_ticks;
    assert_eq!(
        needs.hunger.value,
        ((3 * moving_ticks + 2 * idle_ticks) / 60) as u16
    );
    assert_eq!(
        needs.thirst.value,
        ((6 * moving_ticks + 4 * idle_ticks) / 60) as u16
    );
    assert_eq!(
        needs.rest.value,
        ((3 * moving_ticks + idle_ticks) / 60) as u16
    );
    assert_eq!(needs.exposure.value, (moving_ticks / 60) as u16);

    let mut reached = 0;
    let mut stale = 0;
    while engine.snapshot().tick <= 90_000 {
        engine.tick();
        for outcome in engine.need_threshold_outcomes() {
            if outcome.kind == NeedKind::Thirst {
                reached += usize::from(outcome.outcome == NeedThresholdOutcomeKind::Reached);
                stale += usize::from(outcome.outcome == NeedThresholdOutcomeKind::StaleEvent);
            }
        }
    }
    assert_eq!(reached, 1);
    assert_eq!(stale, 2);
    assert_eq!(engine.agent_views(1).next().unwrap().position, target);
}

#[test]
fn route_waypoints_remain_one_moving_activity_without_need_reschedule_churn() {
    let base = resident_engine();
    let corridor = passable_corridor(&base);
    let mut engine = resident_engine();
    engine
        .initialize_population(
            PopulationInit {
                active_area: engine.world().initial_bounds(),
                population: 1,
            },
            &[corridor[0]],
        )
        .unwrap();
    let route = engine
        .request_route(
            AgentId::new(0),
            RouteRequest {
                destination: corridor[2],
                max_expansions: 16,
            },
        )
        .unwrap();
    while engine.snapshot().tick < route.first_completion.ticks() {
        engine.tick();
    }
    assert_eq!(engine.agent_views(1).next().unwrap().position, corridor[1]);
    assert_eq!(engine.snapshot().scheduled_event_count, 8);
    assert_eq!(
        engine
            .physical_needs(AgentId::new(0))
            .unwrap()
            .thirst
            .rate_per_period,
        6
    );

    while engine.agent_views(1).next().unwrap().position != corridor[2] {
        engine.tick();
    }
    assert_eq!(
        engine
            .physical_needs(AgentId::new(0))
            .unwrap()
            .thirst
            .rate_per_period,
        4
    );
}
