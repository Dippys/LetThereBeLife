use super::*;
use crate::engine::life::TICKS_PER_YEAR;
use crate::life::{Life, SECONDS_PER_YEAR, Sex};

fn two_people() -> Engine {
    let mut engine = resident_engine(64);
    let (first, second) = standable_shelter_site(&engine);
    engine
        .initialize_population(
            PopulationInit {
                active_area: engine.world().initial_bounds(),
                population: 2,
            },
            &[first, second],
        )
        .unwrap();
    engine
}

#[test]
fn the_very_old_die_of_old_age_and_young_adults_do_not() {
    let mut engine = two_people();
    let born = |age: i64| Some(Life {
        born: -((age * SECONDS_PER_YEAR) as i32),
        sex: Sex::Female,
    });
    engine.lives = vec![born(95), born(20)];
    for year in 1..=10 {
        engine.time = SimTime::from_ticks(year * TICKS_PER_YEAR);
        engine.age_people();
    }
    let causes: Vec<_> = engine
        .death_records()
        .iter()
        .map(|record| (record.agent.get(), record.cause))
        .collect();
    assert_eq!(causes, [(0, DeathCause::OldAge)]);
    assert_eq!(engine.life(AgentId::new(1)).unwrap().age, 30);
}

#[test]
fn founders_and_children_get_ages_from_the_seed() {
    let mut engine = two_people();
    engine.set_founders(1);
    let founder = engine.life(AgentId::new(0)).unwrap();
    let child = engine.life(AgentId::new(1)).unwrap();
    assert!((18..40).contains(&founder.age));
    assert!((4..=10).contains(&child.age));
    assert!(engine.strength(AgentId::new(1)) < engine.strength(AgentId::new(0)) + 10);
}
