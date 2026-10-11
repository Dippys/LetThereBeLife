use super::*;
use crate::{PolicyOptions, Species};

/// A small band with full minds, animals, and some history behind it.
fn lived_in_world() -> Engine {
    let mut engine = resident_engine(128);
    let spots: Vec<WorldPosition> = standable_steps(&engine, 8)
        .into_iter()
        .map(|(from, _)| from)
        .collect();
    let area = engine.world().initial_bounds();
    engine
        .initialize_population(
            PopulationInit {
                active_area: area,
                population: spots.len() as u32,
            },
            &spots,
        )
        .unwrap();
    engine.release_wildlife(area, 6, 1);
    engine.set_founders(spots.len() as u32);
    engine
        .activate_physical_policy_with_options(PolicyOptions::full())
        .unwrap();
    engine
}

/// Everything observable about the world, to compare two runs.
fn state(engine: &Engine) -> String {
    let agents: Vec<_> = engine.agent_views(usize::MAX).collect();
    let minds: Vec<_> = agents
        .iter()
        .map(|view| engine.mental_map(view.id))
        .collect();
    let needs: Vec<_> = agents
        .iter()
        .map(|view| engine.physical_needs(view.id).ok())
        .collect();
    let animals: Vec<_> = engine.animal_views().collect();
    let structures: Vec<_> = engine.structure_views(usize::MAX).collect();
    format!(
        "{:?}\n{agents:?}\n{minds:?}\n{needs:?}\n{animals:?}\n{structures:?}",
        engine.snapshot()
    )
}

#[test]
fn a_saved_world_carries_on_exactly_as_if_it_never_stopped() {
    let mut original = lived_in_world();
    for _ in 0..40_000 {
        original.tick();
    }
    let mut file = Vec::new();
    original.save(&mut file).unwrap();
    let mut reopened = Engine::load(file.as_slice()).unwrap();
    assert_eq!(
        state(&reopened),
        state(&original),
        "the same world on opening"
    );
    for _ in 0..40_000 {
        original.tick();
        reopened.tick();
    }
    assert_eq!(
        state(&reopened),
        state(&original),
        "and the same world later"
    );
    assert!(original.snapshot().living_agent_count > 0);
    assert!(original.animal_count(Species::Deer) > 0);
}

#[test]
fn a_save_from_another_version_is_refused() {
    let engine = lived_in_world();
    let mut file = Vec::new();
    engine.save(&mut file).unwrap();
    // The version is the first byte (a small varint): pretend it's 99.
    file[0] = 99;
    assert!(matches!(
        Engine::load(file.as_slice()),
        Err(crate::engine::save::SaveError::Version { found: 99, .. })
    ));
}
