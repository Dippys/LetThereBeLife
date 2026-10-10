//! Eating, drinking, inventory capacity, and action-completion tests.

use super::*;

#[test]
fn eating_consumes_one_food_and_rebases_only_hunger() {
    let mut engine = resident_engine(64);
    let position = standable_steps(&engine, 1)[0].0;
    engine
        .initialize_population(
            PopulationInit {
                active_area: engine.world().initial_bounds(),
                population: 1,
            },
            &[position],
        )
        .unwrap();
    engine.time = SimTime::from_ticks(210_000);
    assert_eq!(
        engine
            .population
            .add_inventory(AgentId::new(0), crate::Material::Berries, 2),
        2
    );
    let before = engine.physical_needs(AgentId::new(0)).unwrap();
    engine.apply_eat(AgentId::new(0)).unwrap();
    let after = engine.physical_needs(AgentId::new(0)).unwrap();
    assert_eq!(
        engine
            .inventory(AgentId::new(0))
            .unwrap()
            .amount(crate::Material::Berries),
        1
    );
    assert_eq!(after.hunger.value, before.hunger.value - EAT_HUNGER_RELIEF);
    assert_eq!(after.thirst.value, before.thirst.value);
    assert_eq!(after.rest.value, before.rest.value);
    assert_eq!(after.exposure.value, before.exposure.value);
    assert_eq!(
        after.next_threshold,
        Some(NeedThreshold {
            kind: NeedKind::Hunger,
            due: SimTime::from_ticks(330_000),
        })
    );
}

#[test]
fn eating_without_food_is_an_explicit_atomic_failure() {
    let mut engine = resident_engine(64);
    let position = standable_steps(&engine, 1)[0].0;
    engine
        .initialize_population(
            PopulationInit {
                active_area: engine.world().initial_bounds(),
                population: 1,
            },
            &[position],
        )
        .unwrap();
    engine.time = SimTime::from_ticks(210_000);
    let before = engine.physical_needs(AgentId::new(0)).unwrap();
    assert_eq!(
        engine.apply_eat(AgentId::new(0)),
        Err(PolicyFailureReason::NoEdibleInventory)
    );
    assert_eq!(engine.physical_needs(AgentId::new(0)).unwrap(), before);
    assert_eq!(
        engine
            .inventory(AgentId::new(0))
            .unwrap()
            .amount(crate::Material::Berries),
        0
    );
}

#[test]
fn inventory_addition_clamps_at_the_per_kind_capacity() {
    let mut engine = resident_engine(64);
    let position = standable_steps(&engine, 1)[0].0;
    engine
        .initialize_population(
            PopulationInit {
                active_area: engine.world().initial_bounds(),
                population: 1,
            },
            &[position],
        )
        .unwrap();
    assert_eq!(
        engine
            .population
            .add_inventory(AgentId::new(0), crate::Material::Wood, u8::MAX),
        INVENTORY_CAPACITY_PER_KIND
    );
    assert_eq!(
        engine
            .population
            .add_inventory(AgentId::new(0), crate::Material::Wood, 1),
        0
    );
    assert_eq!(
        engine
            .inventory(AgentId::new(0))
            .unwrap()
            .amount(crate::Material::Wood),
        INVENTORY_CAPACITY_PER_KIND
    );
}

#[test]
fn action_completion_sequence_exhaustion_settles_idle_without_applying_effects() {
    let mut engine = resident_engine(64);
    let position = standable_steps(&engine, 1)[0].0;
    engine
        .initialize_population(
            PopulationInit {
                active_area: engine.world().initial_bounds(),
                population: 1,
            },
            &[position],
        )
        .unwrap();
    engine
        .population
        .schedule_policy_action(
            &mut engine.scheduler,
            SimTime::ZERO,
            AgentId::new(0),
            PolicyAction {
                goal: PhysicalGoal::GatherMaterial,
                target: position,
                reason: PolicyReason::NoUrgentNeed,
                duration: 1,
            },
        )
        .unwrap();
    engine.scheduler.exhaust_sequence();
    engine.tick();
    assert_eq!(
        engine.agent_views(1).next().unwrap().activity,
        AgentActivity::Idle
    );
    assert!(!engine.physical_policy(AgentId::new(0)).unwrap().committed);
    assert_eq!(engine.modified_resource_count(), 0);
    assert!(engine.policy_diagnostics().iter().any(|diagnostic| {
        diagnostic.kind == PolicyDiagnosticKind::ActionCompleted
            && diagnostic.failure == Some(PolicyFailureReason::EventSequenceExhausted)
    }));
}

#[test]
fn drinking_rejects_ocean_only_and_unloaded_access() {
    let coast = WorldRect {
        min: WorldPosition {
            x: -16_128,
            y: -16_896,
        },
        max: WorldPosition {
            x: -15_104,
            y: -15_872,
        },
    };
    let mut ocean_engine = Engine::new(EngineConfig {
        seed: 1,
        world: WorldConfig::new(64, 64).unwrap(),
        ..EngineConfig::default()
    });
    ocean_engine.command(EngineCommand::GenerateWorldArea(coast));
    let bounds = coast;
    let ocean_access = (bounds.min.y + 1..bounds.max.y - 1)
        .flat_map(|y| (bounds.min.x + 1..bounds.max.x - 1).map(move |x| WorldPosition { x, y }))
        .find(|&position| {
            ocean_engine.world().standability_at(position) == Ok(Standability::Standable)
                && [
                    WorldPosition {
                        x: position.x,
                        y: position.y - 1,
                    },
                    WorldPosition {
                        x: position.x - 1,
                        y: position.y,
                    },
                    WorldPosition {
                        x: position.x + 1,
                        y: position.y,
                    },
                    WorldPosition {
                        x: position.x,
                        y: position.y + 1,
                    },
                ]
                .into_iter()
                .any(|candidate| {
                    ocean_engine.world().water_at(candidate) == Ok(Some(WaterSource::Ocean))
                })
        })
        .expect("seeded resident area should contain an ocean shore");
    ocean_engine
        .initialize_population(
            PopulationInit {
                active_area: bounds,
                population: 1,
            },
            &[ocean_access],
        )
        .unwrap();
    assert_eq!(
        ocean_engine.apply_drink(AgentId::new(0), ocean_access),
        Err(PolicyFailureReason::InvalidWaterAccess)
    );

    let mut unloaded_engine = resident_engine(64);
    let bounds = unloaded_engine.world().initial_bounds();
    let unloaded_access = (bounds.min.y..bounds.max.y)
        .map(|y| WorldPosition {
            x: bounds.max.x - 1,
            y,
        })
        .find(|&position| {
            unloaded_engine.world().standability_at(position) == Ok(Standability::Standable)
                && [
                    position,
                    WorldPosition {
                        x: position.x,
                        y: position.y - 1,
                    },
                    WorldPosition {
                        x: position.x - 1,
                        y: position.y,
                    },
                    WorldPosition {
                        x: position.x,
                        y: position.y + 1,
                    },
                ]
                .into_iter()
                .all(|candidate| {
                    unloaded_engine
                        .world()
                        .water_at(candidate)
                        .is_ok_and(|source| !source.is_some_and(WaterSource::is_drinkable))
                })
        })
        .expect("seeded boundary should contain a dry standable access");
    unloaded_engine
        .initialize_population(
            PopulationInit {
                active_area: bounds,
                population: 1,
            },
            &[unloaded_access],
        )
        .unwrap();
    assert_eq!(
        unloaded_engine.apply_drink(AgentId::new(0), unloaded_access),
        Err(PolicyFailureReason::Unloaded)
    );
}
