//! Public-API gathering depletion and inventory capacity scenarios.

use sim_core::{
    AgentId, Engine, EngineCommand, EngineConfig, InventoryView, PhysicalGoal,
    PolicyDiagnosticKind, PopulationInit, ResourceKind, Standability, WorldConfig, WorldPosition,
    WorldRect,
};

fn resident_engine() -> Engine {
    let mut engine = Engine::new(EngineConfig {
        seed: 42,
        ticks_per_second: 60,
        world: WorldConfig::new(256, 256).unwrap(),
    });
    engine.materialize_initial_area().unwrap();
    engine
}

fn berry_with_two_accesses(engine: &Engine) -> (WorldPosition, [WorldPosition; 2]) {
    let bounds = engine.world().initial_bounds();
    for y in bounds.min.y + 8..bounds.max.y - 8 {
        for x in bounds.min.x + 8..bounds.max.x - 8 {
            let berry = WorldPosition { x, y };
            if !engine.world().resource_at(berry).is_ok_and(|resource| {
                resource.is_some_and(|resource| resource.kind == ResourceKind::Food)
            }) {
                continue;
            }
            let nearby_resource_count = (y - 1..=y + 1)
                .flat_map(|near_y| (x - 1..=x + 1).map(move |near_x| (near_x, near_y)))
                .filter(|&(near_x, near_y)| {
                    engine
                        .world()
                        .resource_at(WorldPosition {
                            x: near_x,
                            y: near_y,
                        })
                        .is_ok_and(|resource| resource.is_some())
                })
                .count();
            if nearby_resource_count != 1 {
                continue;
            }
            let accesses: Vec<_> = [
                berry,
                WorldPosition { x, y: y - 1 },
                WorldPosition { x: x - 1, y },
                WorldPosition { x: x + 1, y },
                WorldPosition { x, y: y + 1 },
            ]
            .into_iter()
            .filter(|position| {
                engine.world().standability_at(*position) == Ok(Standability::Standable)
            })
            .take(2)
            .collect();
            if accesses.len() == 2 {
                return (berry, [accesses[0], accesses[1]]);
            }
        }
    }
    panic!("seeded world should contain a berry resource with two land accesses");
}

fn active_area(berry: WorldPosition) -> WorldRect {
    WorldRect {
        min: WorldPosition {
            x: berry.x - 1,
            y: berry.y - 1,
        },
        max: WorldPosition {
            x: berry.x + 2,
            y: berry.y + 2,
        },
    }
}

fn run_until(engine: &mut Engine, maximum_tick: u64, predicate: impl Fn(&Engine) -> bool) -> u64 {
    while engine.snapshot().tick < maximum_tick {
        engine.tick();
        if predicate(engine) {
            return engine.snapshot().tick;
        }
    }
    panic!("expected action before tick {maximum_tick}");
}

#[test]
fn equal_time_gathering_depletes_one_sparse_delta_without_mutating_base_world() {
    let mut engine = resident_engine();
    let (berry, accesses) = berry_with_two_accesses(&engine);
    let base = engine.world().resource_at(berry).unwrap().unwrap();
    assert_eq!(base.capacity, 12);
    engine
        .initialize_population(
            PopulationInit {
                active_area: active_area(berry),
                population: 2,
            },
            &accesses,
        )
        .unwrap();
    engine.activate_physical_policy().unwrap();

    run_until(&mut engine, 500, |engine| {
        engine.available_resource_at(berry).unwrap().is_none()
    });

    let first = engine.inventory(AgentId::new(0)).unwrap();
    let second = engine.inventory(AgentId::new(1)).unwrap();
    assert_eq!(
        u16::from(first.food) + u16::from(second.food),
        base.capacity
    );
    assert_eq!(engine.modified_resource_count(), 1);
    assert_eq!(engine.world().resource_at(berry).unwrap(), Some(base));
    assert_eq!(engine.available_resource_at(berry).unwrap(), None);
    assert!(
        engine
            .perceive_physical(AgentId::new(0), 2)
            .unwrap()
            .resources
            .iter()
            .all(|resource| resource.position != berry)
    );
    assert!(engine.policy_diagnostics().iter().any(|diagnostic| {
        diagnostic.goal == PhysicalGoal::GatherMaterial
            && diagnostic.kind == PolicyDiagnosticKind::ActionCompleted
            && diagnostic.failure.is_some()
    }));

    engine.command(EngineCommand::Reset);
    assert_eq!(engine.modified_resource_count(), 0);
    assert_eq!(engine.available_resource_at(berry).unwrap(), Some(base));
}

#[test]
fn inventory_capacity_is_enforced_without_heap_items() {
    assert_eq!(std::mem::size_of::<InventoryView>(), 3);
    assert_eq!(sim_core::INVENTORY_CAPACITY_PER_KIND, 32);
    assert_eq!(
        InventoryView::default().remaining_capacity(ResourceKind::Stone),
        32
    );
}
