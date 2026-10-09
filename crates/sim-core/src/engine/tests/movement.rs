//! Movement scheduling, stale events, ordering, and due-event drain tests.

use super::*;

#[test]
fn movement_completes_exactly_at_integer_cost_and_pause_holds_events() {
    let mut engine = resident_engine(64);
    let (from, target) = standable_steps(&engine, 1)[0];
    let cost = engine
        .world()
        .traversal_step(from, target)
        .unwrap()
        .cost()
        .unwrap();
    engine
        .initialize_population(
            PopulationInit {
                active_area: engine.world().initial_bounds(),
                population: 1,
            },
            &[from],
        )
        .unwrap();
    let scheduled = engine.request_move(AgentId::new(0), target).unwrap();
    assert_eq!(scheduled.completes_at, SimTime::from_ticks(u64::from(cost)));
    assert_eq!(
        engine.agent_views(1).next().unwrap().activity,
        AgentActivity::Moving
    );

    engine.command(EngineCommand::SetPaused(true));
    assert_eq!(engine.tick(), TickOutcome::Paused);
    assert_eq!(engine.agent_views(1).next().unwrap().position, from);
    engine.command(EngineCommand::SetPaused(false));
    for _ in 1..cost {
        engine.tick();
        assert_eq!(engine.agent_views(1).next().unwrap().position, from);
    }
    engine.tick();
    assert_eq!(engine.agent_views(1).next().unwrap().position, target);
    assert_eq!(engine.movement_outcomes().len(), 1);
    assert_eq!(
        engine.movement_outcomes()[0].kind,
        MovementOutcomeKind::Moved
    );
}

#[test]
fn superseded_equal_time_movement_is_stale_and_cannot_move_twice() {
    let mut engine = resident_engine(64);
    let (from, target) = standable_steps(&engine, 1)[0];
    engine
        .initialize_population(
            PopulationInit {
                active_area: engine.world().initial_bounds(),
                population: 1,
            },
            &[from],
        )
        .unwrap();
    let due = engine
        .request_move(AgentId::new(0), target)
        .unwrap()
        .completes_at;
    assert_eq!(
        engine
            .request_move(AgentId::new(0), target)
            .unwrap()
            .completes_at,
        due
    );
    for _ in 0..due.ticks() {
        engine.tick();
    }
    assert_eq!(
        engine
            .movement_outcomes()
            .iter()
            .map(|outcome| outcome.kind)
            .collect::<Vec<_>>(),
        [MovementOutcomeKind::StaleEvent, MovementOutcomeKind::Moved]
    );
    assert_eq!(engine.agent_views(1).next().unwrap().position, target);
}

#[test]
fn movement_rejections_are_typed_and_do_not_mutate_agent_or_world() {
    let mut engine = resident_engine(64);
    let (from, target) = standable_steps(&engine, 1)[0];
    engine
        .initialize_population(
            PopulationInit {
                active_area: engine.world().initial_bounds(),
                population: 1,
            },
            &[from],
        )
        .unwrap();
    engine.spawn_object(SpawnKind::Water, target).unwrap();
    let revision = engine.world().revision();
    assert_eq!(
        engine.request_move(AgentId::new(0), from),
        Err(MoveRequestError::InvalidStep)
    );
    assert_eq!(
        engine.request_move(AgentId::new(99), target),
        Err(MoveRequestError::MissingAgent)
    );
    assert_eq!(
        engine.request_move(AgentId::new(0), target),
        Err(MoveRequestError::Blocked(TraversalKind::BlockedByWater))
    );
    assert_eq!(
        engine.request_move(
            AgentId::new(0),
            WorldPosition {
                x: WORLD_HALF_EXTENT,
                y: from.y,
            },
        ),
        Err(MoveRequestError::OutsideWorld)
    );
    assert_eq!(engine.agent_views(1).next().unwrap().position, from);
    assert_eq!(
        engine.agent_views(1).next().unwrap().activity,
        AgentActivity::Idle
    );
    assert_eq!(engine.world().revision(), revision);

    engine.population.mark_dead(AgentId::new(0));
    assert_eq!(
        engine.request_move(AgentId::new(0), target),
        Err(MoveRequestError::DeadAgent)
    );

    let mut bounded = resident_engine(64);
    let (bounded_from, bounded_target) = standable_steps(&bounded, 1)[0];
    bounded
        .initialize_population(
            PopulationInit {
                active_area: WorldRect {
                    min: bounded_from,
                    max: WorldPosition {
                        x: bounded_from.x + 1,
                        y: bounded_from.y + 1,
                    },
                },
                population: 1,
            },
            &[bounded_from],
        )
        .unwrap();
    assert_eq!(
        bounded.request_move(AgentId::new(0), bounded_target),
        Err(MoveRequestError::OutsideActiveArea)
    );
}

#[test]
fn equal_time_events_apply_in_agent_id_order_not_insertion_order() {
    let base = resident_engine(64);
    let candidates = standable_steps(&base, 32);
    let (first, second) = candidates
        .iter()
        .enumerate()
        .find_map(|(index, &left)| {
            let left_cost = base.world().traversal_step(left.0, left.1).ok()?.cost()?;
            candidates[index + 1..].iter().copied().find_map(|right| {
                (base.world().traversal_step(right.0, right.1).ok()?.cost()? == left_cost
                    && left.0 != right.0
                    && left.0 != right.1
                    && left.1 != right.0
                    && left.1 != right.1)
                    .then_some((left, right))
            })
        })
        .expect("seeded test area should contain two equal-cost steps");
    let steps = [first, second];
    let origins = [steps[0].0, steps[1].0];
    let mut forward = resident_engine(64);
    let mut reverse = resident_engine(64);
    for engine in [&mut forward, &mut reverse] {
        engine
            .initialize_population(
                PopulationInit {
                    active_area: engine.world().initial_bounds(),
                    population: 2,
                },
                &origins,
            )
            .unwrap();
    }
    forward.request_move(AgentId::new(0), steps[0].1).unwrap();
    forward.request_move(AgentId::new(1), steps[1].1).unwrap();
    reverse.request_move(AgentId::new(1), steps[1].1).unwrap();
    reverse.request_move(AgentId::new(0), steps[0].1).unwrap();
    for _ in 0..64 {
        forward.tick();
        reverse.tick();
    }
    assert_eq!(
        forward.agent_views(10).collect::<Vec<_>>(),
        reverse.agent_views(10).collect::<Vec<_>>()
    );
    assert_eq!(forward.snapshot(), reverse.snapshot());
}

#[test]
fn sequence_exhaustion_settles_due_route_without_stranding_moving_activity() {
    let mut engine = resident_engine(64);
    let pair = standable_steps(&engine, 1)[0];
    engine
        .initialize_population(
            PopulationInit {
                active_area: engine.world().initial_bounds(),
                population: 1,
            },
            &[pair.0],
        )
        .unwrap();
    let route = engine
        .request_route(
            AgentId::new(0),
            RouteRequest {
                destination: pair.1,
                max_expansions: 8,
            },
        )
        .unwrap();
    engine.scheduler.exhaust_sequence();
    while engine.snapshot().tick < route.first_completion.ticks() {
        engine.tick();
    }
    let view = engine.agent_views(1).next().unwrap();
    assert_eq!(view.position, pair.0);
    assert_eq!(view.activity, AgentActivity::Idle);
    assert_eq!(
        engine
            .physical_needs(view.id)
            .unwrap()
            .thirst
            .rate_per_period,
        4
    );
    assert_eq!(
        engine.route_outcomes().last().unwrap().kind,
        RouteOutcomeKind::EventSequenceExhausted
    );
}

#[test]
fn due_event_drain_is_bounded_and_reports_backlog() {
    let mut engine = Engine::default();
    for sequence in 0..=MAX_DUE_EVENTS_PER_TICK {
        engine
            .scheduler
            .schedule_movement(
                SimTime::from_ticks(1),
                AgentId::new(sequence as u32),
                0,
                agent::CompactPosition { x: 0, y: 0 },
            )
            .unwrap();
    }
    assert_eq!(
        engine.tick(),
        TickOutcome::Advanced {
            time: SimTime::from_ticks(1),
            processed_events: MAX_DUE_EVENTS_PER_TICK as u16,
            due_backlog: true,
        }
    );
    assert_eq!(engine.movement_outcomes().len(), MAX_DUE_EVENTS_PER_TICK);
    assert_eq!(engine.snapshot().scheduled_event_count, 1);
}

#[test]
fn simulation_time_exhaustion_is_typed_and_does_not_repeat_due_work() {
    let mut engine = Engine {
        time: SimTime::from_ticks(u64::MAX),
        ..Engine::default()
    };
    assert_eq!(engine.tick(), TickOutcome::TimeExhausted);
    assert_eq!(engine.snapshot().tick, u64::MAX);
}
