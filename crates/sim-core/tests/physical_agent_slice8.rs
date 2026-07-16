use sim_core::{
    AgentId, Engine, EngineCommand, EngineConfig, InitialInventoryError, InventoryView,
    PopulationInit, Standability, WorldConfig,
};

#[test]
fn initial_supplies_are_capped_pre_policy_setup_and_reset_cleanly() {
    let mut engine = Engine::new(EngineConfig {
        seed: 1,
        world: WorldConfig::new(64, 64).unwrap(),
        ..EngineConfig::default()
    });
    engine.materialize_initial_area().unwrap();
    let spawn = engine
        .world()
        .cells()
        .map(|(position, _)| position)
        .find(|&position| engine.world().standability_at(position) == Ok(Standability::Standable))
        .unwrap();
    let init = PopulationInit {
        active_area: engine.world().initial_bounds(),
        population: 1,
    };
    engine.initialize_population(init, &[spawn]).unwrap();

    assert_eq!(
        engine.set_initial_inventory(
            AgentId::new(0),
            InventoryView {
                food: 33,
                ..InventoryView::default()
            }
        ),
        Err(InitialInventoryError::AmountExceedsCapacity)
    );
    let supplies = InventoryView {
        food: 32,
        wood: 8,
        stone: 0,
    };
    engine
        .set_initial_inventory(AgentId::new(0), supplies)
        .unwrap();
    assert_eq!(engine.inventory(AgentId::new(0)), Some(supplies));

    engine.activate_physical_policy().unwrap();
    assert_eq!(
        engine.set_initial_inventory(AgentId::new(0), InventoryView::default()),
        Err(InitialInventoryError::PolicyActive)
    );

    engine.command(EngineCommand::Reset);
    engine.initialize_population(init, &[spawn]).unwrap();
    assert_eq!(
        engine.inventory(AgentId::new(0)),
        Some(InventoryView::default())
    );
    engine.tick();
    assert_eq!(
        engine.set_initial_inventory(AgentId::new(0), supplies),
        Err(InitialInventoryError::SimulationAdvanced)
    );
}
