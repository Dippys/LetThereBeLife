//! Autonomous policy tests: failure mapping, contested water access, perception filtering, and exploration.

use super::*;

#[test]
fn policy_failure_mapping_preserves_typed_runtime_categories() {
    assert_eq!(
        perception_failure(PerceptionError::Unloaded),
        PolicyFailureReason::Unloaded
    );
    assert_eq!(
        request_failure(RouteRequestError::Occupied(AgentId::new(7))),
        PolicyFailureReason::Occupied
    );
    assert_eq!(
        request_failure(RouteRequestError::NoPath { expansions: 9 }),
        PolicyFailureReason::NoPath
    );
    assert_eq!(
        request_failure(RouteRequestError::BudgetExhausted { expansions: 9 }),
        PolicyFailureReason::RouteBudgetExhausted
    );
    assert_eq!(
        request_failure(RouteRequestError::OutsideActiveArea),
        PolicyFailureReason::OutsideActiveArea
    );
    assert_eq!(
        route_failure(RouteOutcomeKind::Blocked(TraversalKind::BlockedByWater)),
        PolicyFailureReason::TargetUnavailable
    );
    assert_eq!(
        move_failure(MoveRequestError::EventSequenceExhausted),
        PolicyFailureReason::EventSequenceExhausted
    );
    assert_eq!(
        move_failure(MoveRequestError::RescheduleLimit),
        PolicyFailureReason::RescheduleLimit
    );
    assert_eq!(
        move_failure(MoveRequestError::TimeOverflow),
        PolicyFailureReason::TimeOverflow
    );
}

#[test]
fn crowded_water_seekers_claim_distinct_access_and_both_drink() {
    let mut engine = resident_engine(128);
    let bounds = engine.world().initial_bounds();
    let water = (bounds.min.y + 4..bounds.max.y - 4)
        .flat_map(|y| (bounds.min.x + 4..bounds.max.x - 4).map(move |x| WorldPosition { x, y }))
        .find(|center| {
            let patch = WorldRect {
                min: WorldPosition {
                    x: center.x - 3,
                    y: center.y - 4,
                },
                max: WorldPosition {
                    x: center.x + 4,
                    y: center.y + 4,
                },
            };
            (patch.min.y..patch.max.y).all(|y| {
                (patch.min.x..patch.max.x).all(|x| {
                    let position = WorldPosition { x, y };
                    engine.world().standability_at(position) == Ok(Standability::Standable)
                        && engine.available_resource_at(position) == Ok(None)
                        && [(1, 0), (0, 1)].into_iter().all(|(dx, dy)| {
                            let neighbor = WorldPosition {
                                x: x + dx,
                                y: y + dy,
                            };
                            !patch.contains(neighbor)
                                || engine
                                    .world()
                                    .traversal_step(position, neighbor)
                                    .is_ok_and(|step| step.is_passable())
                        })
                })
            })
        })
        .expect("seeded resident terrain should contain a small passable patch");
    engine.spawn_object(SpawnKind::Water, water).unwrap();
    let positions = [
        WorldPosition {
            x: water.x,
            y: water.y - 3,
        },
        WorldPosition {
            x: water.x,
            y: water.y - 4,
        },
    ];
    engine
        .initialize_population(
            PopulationInit {
                active_area: bounds,
                population: 2,
            },
            &positions,
        )
        .unwrap();
    for agent in [AgentId::new(0), AgentId::new(1)] {
        engine
            .population
            .set_need_value_for_test(agent, NeedKind::Thirst, 6_000, SimTime::ZERO);
    }
    engine.activate_physical_policy_with_exploration().unwrap();
    engine.tick();

    let first = engine.physical_policy(AgentId::new(0)).unwrap();
    let second = engine.physical_policy(AgentId::new(1)).unwrap();
    assert_eq!(first.goal, PhysicalGoal::SeekWater);
    assert_eq!(second.goal, PhysicalGoal::SeekWater);
    assert_ne!(first.target, second.target);
    assert_eq!(first.retry_count, 0);
    assert_eq!(second.retry_count, 0);

    let mut drank = [false; 2];
    for _ in 0..2_000 {
        engine.tick();
        for diagnostic in engine.policy_diagnostics() {
            if diagnostic.kind == PolicyDiagnosticKind::ActionCompleted
                && diagnostic.goal == PhysicalGoal::Drink
                && diagnostic.failure.is_none()
            {
                drank[diagnostic.agent.get() as usize] = true;
            }
        }
        if drank.into_iter().all(|completed| completed) {
            break;
        }
    }
    assert_eq!(drank, [true, true]);

    let anchored = [
        engine.agent_views(2).next().unwrap().position,
        engine.agent_views(2).nth(1).unwrap().position,
    ];
    for _ in 0..1_000 {
        engine.tick();
    }
    for (index, position) in anchored.into_iter().enumerate() {
        let agent = AgentId::new(index as u32);
        assert_eq!(engine.agent_views(2).nth(index).unwrap().position, position);
        assert!(matches!(
            engine.physical_policy(agent).unwrap().goal,
            PhysicalGoal::Wait | PhysicalGoal::Drink
        ));
        assert!(engine.has_adjacent_drinkable_water(position).unwrap());
    }
}

#[test]
fn perception_excludes_terrain_disconnected_cells_from_autonomous_targets() {
    let mut engine = resident_engine(128);
    let bounds = engine.world().initial_bounds();
    let center = (bounds.min.y + 3..bounds.max.y - 3)
        .flat_map(|y| (bounds.min.x + 3..bounds.max.x - 3).map(move |x| WorldPosition { x, y }))
        .find(|center| {
            let patch = WorldRect {
                min: WorldPosition {
                    x: center.x - 2,
                    y: center.y - 2,
                },
                max: WorldPosition {
                    x: center.x + 3,
                    y: center.y + 3,
                },
            };
            (patch.min.y..patch.max.y).all(|y| {
                (patch.min.x..patch.max.x).all(|x| {
                    let position = WorldPosition { x, y };
                    engine.world().standability_at(position) == Ok(Standability::Standable)
                        && engine.available_resource_at(position) == Ok(None)
                        && [(1, 0), (0, 1)].into_iter().all(|(dx, dy)| {
                            let neighbor = WorldPosition {
                                x: x + dx,
                                y: y + dy,
                            };
                            !patch.contains(neighbor)
                                || engine
                                    .world()
                                    .traversal_step(position, neighbor)
                                    .is_ok_and(TraversalStep::is_passable)
                        })
                })
            })
        })
        .expect("seeded terrain should contain a resource-free passable patch");
    for position in [
        WorldPosition {
            x: center.x,
            y: center.y - 1,
        },
        WorldPosition {
            x: center.x - 1,
            y: center.y,
        },
        WorldPosition {
            x: center.x + 1,
            y: center.y,
        },
        WorldPosition {
            x: center.x,
            y: center.y + 1,
        },
    ] {
        engine.spawn_object(SpawnKind::Water, position).unwrap();
    }
    engine
        .initialize_population(
            PopulationInit {
                active_area: bounds,
                population: 1,
            },
            &[center],
        )
        .unwrap();

    let perception = engine.perceive_physical(AgentId::new(0), 2).unwrap();
    assert!(perception.traversable_cells.len() > 1);
    assert_eq!(perception.reachable_cells, [center]);
    let (_, needs, inventory) = engine
        .population
        .policy_context(AgentId::new(0), SimTime::ZERO)
        .unwrap();
    let (selection, _) = select_with_exploration(
        center,
        needs,
        inventory,
        &perception,
        Some(ExplorationHeading::North),
    );
    assert_eq!(selection.goal, PhysicalGoal::Wait);
    assert_eq!(selection.target, Some(center));
}

#[test]
fn isolated_agent_explores_a_full_local_window_without_route_backoff() {
    let mut engine = resident_engine(128);
    let bounds = engine.world().initial_bounds();
    let origin = (bounds.min.y + 8..bounds.max.y - 8)
        .flat_map(|y| (bounds.min.x + 8..bounds.max.x - 8).map(move |x| WorldPosition { x, y }))
        .find(|origin| {
            let patch = WorldRect {
                min: WorldPosition {
                    x: origin.x - 8,
                    y: origin.y - 8,
                },
                max: WorldPosition {
                    x: origin.x + 9,
                    y: origin.y + 9,
                },
            };
            (patch.min.y..patch.max.y).all(|y| {
                (patch.min.x..patch.max.x).all(|x| {
                    let position = WorldPosition { x, y };
                    engine.world().standability_at(position) == Ok(Standability::Standable)
                        && engine.available_resource_at(position) == Ok(None)
                        && engine.world().water_at(position) == Ok(None)
                        && [(1, 0), (0, 1)].into_iter().all(|(dx, dy)| {
                            let neighbor = WorldPosition {
                                x: x + dx,
                                y: y + dy,
                            };
                            !patch.contains(neighbor)
                                || engine
                                    .world()
                                    .traversal_step(position, neighbor)
                                    .is_ok_and(TraversalStep::is_passable)
                        })
                })
            })
        })
        .expect("seeded terrain should contain a clear local exploration window");
    engine
        .initialize_population(
            PopulationInit {
                active_area: bounds,
                population: 1,
            },
            &[origin],
        )
        .unwrap();
    engine.activate_physical_policy_with_exploration().unwrap();

    engine.tick();

    let policy = engine.physical_policy(AgentId::new(0)).unwrap();
    assert_eq!(policy.goal, PhysicalGoal::Explore);
    assert!(policy.committed);
    assert_eq!(policy.retry_count, 0);
    assert_eq!(
        engine.agent_views(1).next().unwrap().activity,
        AgentActivity::Moving
    );
    assert!(engine.policy_diagnostics().iter().all(|diagnostic| {
        diagnostic.failure != Some(PolicyFailureReason::RouteBudgetExhausted)
    }));
}
