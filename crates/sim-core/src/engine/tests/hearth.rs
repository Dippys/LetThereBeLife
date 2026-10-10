use super::*;
use crate::{InventoryView, Material, NeedKind, PolicyOptions, PolicyReason, StructureKind};

/// A builder carrying a hearth's stone and wood next to a free site, and a
/// child watching who has never seen fire.
fn builder_and_watcher() -> (Engine, WorldPosition) {
    let mut engine = resident_engine(64);
    let steps = standable_steps(&engine, 4);
    let (builder, site) = steps[0];
    let watcher = steps
        .iter()
        .map(|(from, _)| *from)
        .find(|from| *from != builder && *from != site)
        .expect("room for a watcher");
    let area = engine.world().initial_bounds();
    engine
        .initialize_population(
            PopulationInit {
                active_area: area,
                population: 2,
            },
            &[builder, watcher],
        )
        .unwrap();
    engine
        .set_initial_inventory(
            AgentId::new(0),
            InventoryView::of(&[(Material::Stone, 3), (Material::Wood, 2)]),
        )
        .unwrap();
    engine.policy_options = PolicyOptions::full();
    engine.minds.set_founders(1);
    (engine, site)
}

#[test]
fn a_hearth_is_built_from_stone_and_wood_and_is_not_a_shelter() {
    let (mut engine, site) = builder_and_watcher();
    let hearth = engine
        .start_build(
            AgentId::new(0),
            site,
            PolicyReason::NoUrgentNeed,
            StructureKind::Hearth,
        )
        .unwrap();
    assert_eq!(hearth.kind, StructureKind::Hearth);
    let left = engine.population.inventory(AgentId::new(0)).unwrap();
    assert_eq!(
        (left.amount(Material::Stone), left.amount(Material::Wood)),
        (0, 0)
    );
    engine.apply_build_completion(AgentId::new(0)).unwrap();
    let builder = engine.population.view(AgentId::new(0)).unwrap().position;
    assert!(engine.structures.hearth_beside(builder));
    assert!(
        !engine.structures.is_sheltered_access(builder),
        "a fire is no roof"
    );
}

#[test]
fn warming_up_eases_the_cold_and_teaches_whoever_watches() {
    let (mut engine, site) = builder_and_watcher();
    engine
        .start_build(
            AgentId::new(0),
            site,
            PolicyReason::NoUrgentNeed,
            StructureKind::Hearth,
        )
        .unwrap();
    engine.apply_build_completion(AgentId::new(0)).unwrap();
    engine.population.set_need_value_for_test(
        AgentId::new(0),
        NeedKind::Exposure,
        5_000,
        engine.time,
    );
    assert!(!engine.minds.get_mut(AgentId::new(1)).crafts.knows_hearths());

    engine.apply_warm_up(AgentId::new(0)).unwrap();

    let exposure = engine
        .population
        .needs_view(AgentId::new(0), engine.time)
        .unwrap()
        .exposure
        .value;
    assert!(exposure <= 5_000 - crate::HEARTH_WARMTH + 10, "{exposure}");
    assert!(engine.minds.get_mut(AgentId::new(0)).crafts.knows_hearths());
    assert!(
        engine.minds.get_mut(AgentId::new(1)).crafts.knows_hearths(),
        "the child saw someone warm their hands"
    );
}

#[test]
fn warming_up_needs_a_hearth_beside_you() {
    let (mut engine, _) = builder_and_watcher();
    assert_eq!(
        engine.apply_warm_up(AgentId::new(0)),
        Err(crate::PolicyFailureReason::TargetUnavailable)
    );
}
