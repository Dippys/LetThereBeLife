//! Public-API route planning (shared destinations, budgets, no-path) and bounded perception scenarios.

use sim_core::{
    AgentId, Engine, EngineConfig, PopulationInit, RouteOutcomeKind, RouteRequest,
    RouteRequestError, Standability, TraversalStep, WorldConfig, WorldPosition, WorldRect,
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

fn equal_cost_contention(engine: &Engine) -> (WorldPosition, [WorldPosition; 2]) {
    let bounds = engine.world().initial_bounds();
    for y in bounds.min.y + 1..bounds.max.y - 1 {
        for x in bounds.min.x + 1..bounds.max.x - 1 {
            let target = WorldPosition { x, y };
            if engine.world().standability_at(target) != Ok(Standability::Standable) {
                continue;
            }
            let mut origins = Vec::new();
            for (dx, dy) in [(0, -1), (-1, 0), (1, 0), (0, 1)] {
                let origin = WorldPosition {
                    x: x + dx,
                    y: y + dy,
                };
                if let Ok(step) = engine.world().traversal_step(origin, target)
                    && let Some(cost) = step.cost()
                {
                    origins.push((origin, cost));
                }
            }
            for left in 0..origins.len() {
                for right in left + 1..origins.len() {
                    if origins[left].1 == origins[right].1 {
                        return (target, [origins[left].0, origins[right].0]);
                    }
                }
            }
        }
    }
    panic!("seeded world should contain equal-cost contention geometry");
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
    panic!("seeded world should contain a three-cell passable corridor");
}

#[test]
fn equal_time_routes_can_share_a_destination_and_preserve_both_agents() {
    let base = resident_engine();
    let (target, origins) = equal_cost_contention(&base);
    let mut forward = resident_engine();
    let mut reverse = resident_engine();
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
    let request = RouteRequest {
        destination: target,
        max_expansions: 32,
    };
    forward.request_route(AgentId::new(0), request).unwrap();
    forward.request_route(AgentId::new(1), request).unwrap();
    reverse.request_route(AgentId::new(1), request).unwrap();
    reverse.request_route(AgentId::new(0), request).unwrap();

    let mut forward_routes = Vec::new();
    let mut reverse_routes = Vec::new();
    for _ in 0..64 {
        forward.tick();
        reverse.tick();
        forward_routes.extend(forward.route_outcomes().iter().map(|outcome| outcome.kind));
        reverse_routes.extend(reverse.route_outcomes().iter().map(|outcome| outcome.kind));
    }
    let forward_views: Vec<_> = forward.agent_views(2).collect();
    assert_eq!(forward_views, reverse.agent_views(2).collect::<Vec<_>>());
    assert_eq!(forward_views[0].position, target);
    assert_eq!(forward_views[1].position, target);
    assert_eq!(forward_routes, reverse_routes);
    assert_eq!(forward_routes, [RouteOutcomeKind::Arrived; 2]);
    assert_eq!(forward.snapshot(), reverse.snapshot());

    let perception = forward.perceive_physical(AgentId::new(0), 2).unwrap();
    assert_eq!(perception.agents.len(), 2);
    assert!(perception.agents.windows(2).all(|pair| {
        (pair[0].position.y, pair[0].position.x, pair[0].id)
            < (pair[1].position.y, pair[1].position.x, pair[1].id)
    }));
}

#[test]
fn bounded_routes_distinguish_budget_exhaustion_and_arrival() {
    let base = resident_engine();
    let corridor = passable_corridor(&base);
    let active_area = WorldRect {
        min: corridor[0],
        max: WorldPosition {
            x: corridor[2].x + 1,
            y: corridor[2].y + 1,
        },
    };

    let mut budgeted = resident_engine();
    budgeted
        .initialize_population(
            PopulationInit {
                active_area,
                population: 1,
            },
            &[corridor[0]],
        )
        .unwrap();
    assert_eq!(
        budgeted.request_route(
            AgentId::new(0),
            RouteRequest {
                destination: corridor[2],
                max_expansions: 1,
            },
        ),
        Err(RouteRequestError::BudgetExhausted { expansions: 1 })
    );
    let scheduled = budgeted
        .request_route(
            AgentId::new(0),
            RouteRequest {
                destination: corridor[2],
                max_expansions: 3,
            },
        )
        .unwrap();
    assert_eq!(scheduled.destination, corridor[2]);
    let mut route_outcomes = Vec::new();
    for _ in 0..128 {
        budgeted.tick();
        route_outcomes.extend(budgeted.route_outcomes().iter().map(|outcome| outcome.kind));
    }
    assert_eq!(
        budgeted.agent_views(1).next().unwrap().position,
        corridor[2]
    );
    assert_eq!(route_outcomes, [RouteOutcomeKind::Arrived]);
}

#[test]
fn perception_is_bounded_row_major_and_reports_objective_facts() {
    let mut engine = resident_engine();
    let corridor = passable_corridor(&engine);
    engine
        .initialize_population(
            PopulationInit {
                active_area: engine.world().initial_bounds(),
                population: 2,
            },
            &[corridor[0], corridor[1]],
        )
        .unwrap();
    let perception = engine.perceive_physical(AgentId::new(0), 3).unwrap();
    assert!(perception.area.contains(corridor[0]));
    assert_eq!(perception.agents.len(), 2);
    assert!(
        perception
            .traversable_cells
            .windows(2)
            .all(|cells| { (cells[0].y, cells[0].x) < (cells[1].y, cells[1].x) })
    );
    assert!(
        perception
            .drinkable_water
            .iter()
            .all(|water| water.source.is_drinkable())
    );
    assert_eq!(
        engine.perceive_physical(AgentId::new(0), 32),
        Err(sim_core::PerceptionError::RadiusTooLarge {
            requested: 32,
            maximum: 31,
        })
    );
    let rectangle = WorldRect {
        min: corridor[0],
        max: WorldPosition {
            x: corridor[2].x + 1,
            y: corridor[2].y + 1,
        },
    };
    assert_eq!(
        engine
            .perceive_physical_area(AgentId::new(0), rectangle)
            .unwrap()
            .area,
        rectangle
    );
    assert_eq!(
        engine.perceive_physical_area(
            AgentId::new(0),
            WorldRect {
                min: corridor[0],
                max: WorldPosition {
                    x: corridor[0].x + 64,
                    y: corridor[0].y + 64,
                },
            },
        ),
        Err(sim_core::PerceptionError::AreaTooLarge {
            requested: 4_096,
            maximum: 3_969,
        })
    );
}
