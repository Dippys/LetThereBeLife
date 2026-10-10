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
    let born = |age: i64| {
        Some(Life {
            born: -((age * SECONDS_PER_YEAR) as i32),
            sex: Sex::Female,
            inherited: None,
        })
    };
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

#[test]
fn seeing_the_body_of_family_brings_grief() {
    let mut engine = two_people();
    engine.policy_options = crate::PolicyOptions::full();
    engine
        .activate_physical_policy_with_options(crate::PolicyOptions::full())
        .unwrap();
    engine.bond(AgentId::new(1), AgentId::new(0));
    engine.lives = vec![
        Some(Life {
            born: -((120 * SECONDS_PER_YEAR) as i32),
            sex: Sex::Male,
            inherited: None,
        }),
        None,
    ];
    engine.time = SimTime::from_ticks(TICKS_PER_YEAR);
    engine.age_people();
    assert_eq!(engine.death_records()[0].cause, DeathCause::OldAge);
    let mut mourned = Vec::new();
    for _ in 0..crate::PHYSICAL_POLICY_IDLE_RECHECK_TICKS * 2 {
        engine.tick();
        mourned.extend(
            engine
                .grief_events()
                .iter()
                .map(|grief| (grief.agent, grief.lost)),
        );
    }
    assert_eq!(mourned, [(AgentId::new(1), AgentId::new(0))]);
    let mind = engine.minds.get(AgentId::new(1)).unwrap();
    assert!(mind.grief_until > 0);
    assert_eq!(
        mind.social.slot_of(AgentId::new(0)),
        None,
        "mourned, then let go"
    );
}

/// Two adults of the other sex who know each other this well.
fn acquainted(familiarity_sightings: u32, raised_together: bool) -> Engine {
    let mut engine = two_people();
    engine.policy_options = crate::PolicyOptions::full();
    let adult = |sex| {
        Some(Life {
            born: -((25 * SECONDS_PER_YEAR) as i32),
            sex,
            inherited: None,
        })
    };
    engine.lives = vec![adult(Sex::Female), adult(Sex::Male)];
    for (me, other) in [(0, 1), (1, 0)] {
        let position = engine
            .population
            .view(AgentId::new(other))
            .unwrap()
            .position;
        let social = &mut engine.minds.get_mut(AgentId::new(me)).social;
        for _ in 0..familiarity_sightings {
            social.notice(AgentId::new(other), position, 1);
        }
        if raised_together {
            let slot = social.slot_of(AgentId::new(other)).unwrap();
            social.mark_raised_together(slot);
        }
    }
    engine
}

fn decide(engine: &mut Engine, agent: u32) {
    let perception = engine
        .perceive_physical(AgentId::new(agent), crate::PHYSICAL_POLICY_RADIUS)
        .unwrap();
    engine.pair_up(AgentId::new(agent), &perception);
}

#[test]
fn people_who_know_each_other_well_become_a_couple() {
    let mut engine = acquainted(60, false);
    decide(&mut engine, 0);
    assert_eq!(engine.couple_events().len(), 1);
    assert_eq!(engine.partner_of(AgentId::new(0)), Some(AgentId::new(1)));
    assert_eq!(engine.partner_of(AgentId::new(1)), Some(AgentId::new(0)));

    // Barely acquainted people don't.
    let mut engine = acquainted(5, false);
    decide(&mut engine, 0);
    assert_eq!(engine.partner_of(AgentId::new(0)), None);
}

#[test]
fn people_raised_together_do_not_pair_unless_long_alone() {
    let mut engine = acquainted(60, true);
    decide(&mut engine, 0);
    assert_eq!(
        engine.partner_of(AgentId::new(0)),
        None,
        "they grew up as siblings"
    );

    // Years with nobody else to pair with wear the feeling down.
    engine.time = SimTime::from_ticks(10 * TICKS_PER_YEAR);
    for agent in [0, 1] {
        engine.minds.get_mut(AgentId::new(agent)).last_eligible_seen = 0;
    }
    engine.lives = engine
        .lives
        .iter()
        .map(|life| {
            life.map(|life| Life {
                born: life.born - 10 * SECONDS_PER_YEAR as i32,
                ..life
            })
        })
        .collect();
    decide(&mut engine, 0);
    assert_eq!(engine.partner_of(AgentId::new(0)), Some(AgentId::new(1)));
}

#[test]
fn a_couple_has_a_baby_who_later_walks_and_knows_its_family() {
    let mut engine = acquainted(60, false);
    engine
        .activate_physical_policy_with_options(crate::PolicyOptions::full())
        .unwrap();
    decide(&mut engine, 0);
    assert_eq!(engine.partner_of(AgentId::new(0)), Some(AgentId::new(1)));
    // Try until she conceives (each try is one decision spent together).
    let mut tries = 0;
    while engine.motherhood(AgentId::new(0)).is_none() {
        engine.time = SimTime::from_ticks(engine.time.ticks() + 7);
        engine.try_conceive(AgentId::new(0), AgentId::new(1));
        tries += 1;
        assert!(tries < 20_000, "never conceived");
    }
    assert!(engine.motherhood(AgentId::new(0)).unwrap().pregnant);

    // Nine months, then three years carried.
    let check = crate::engine::births::FAMILY_CHECK_TICKS;
    let start = engine.time.ticks().div_ceil(check) * check;
    let mut walking = None;
    let mut step = 0;
    while walking.is_none() {
        step += 1;
        engine.time = SimTime::from_ticks(start + step * crate::engine::births::FAMILY_CHECK_TICKS);
        // Keep the mother fed so nursing doesn't starve her in this short-cut.
        engine.population.set_need_value_for_test(
            AgentId::new(0),
            crate::NeedKind::Hunger,
            0,
            engine.time,
        );
        engine.tend_families();
        walking = engine
            .family_events()
            .iter()
            .find_map(|event| match *event {
                crate::FamilyEvent::Walking { child, .. } => Some(child),
                _ => None,
            });
        engine.family_events.clear();
        assert!(step < 3_000, "the child never walked");
    }
    let child = walking.unwrap();
    assert_eq!(child, AgentId::new(2));
    let life = engine.life(child).unwrap();
    assert_eq!(life.age, crate::WEANING_AGE);
    let mind = engine.minds.get(child).unwrap();
    assert_eq!(mind.parent, Some(AgentId::new(0)), "it follows its mother");
    assert!(
        mind.lexicon.produce(crate::Concept::Water).is_none(),
        "born with no words"
    );
    assert_eq!(
        mind.social.tie_with(AgentId::new(1)),
        Some(crate::Tie::Parent)
    );
    let mother = &engine.minds.get(AgentId::new(0)).unwrap().social;
    assert_eq!(mother.tie_with(child), Some(crate::Tie::Child));
    assert!(engine.motherhood(AgentId::new(0)).is_none());
}

#[test]
fn a_dwindling_band_is_joined_by_newcomers_with_their_own_words() {
    let mut engine = two_people();
    engine.policy_options = crate::PolicyOptions::full();
    engine.set_founders(2);
    engine.time = SimTime::from_ticks(TICKS_PER_YEAR);
    engine.age_people();
    let arrived = engine
        .family_events()
        .iter()
        .find_map(|event| match *event {
            crate::FamilyEvent::Arrived { woman, man } => Some((woman, man)),
            _ => None,
        });
    let (woman, man) = arrived.expect("newcomers arrived");
    assert_eq!(engine.life(woman).unwrap().sex, Sex::Female);
    assert_eq!(engine.life(man).unwrap().sex, Sex::Male);
    assert!(engine.life(man).unwrap().age >= crate::ADULT_AGE);
    assert_eq!(engine.partner_of(woman), Some(man));
    assert!(engine.minds.is_founder(woman), "they bring their own lore");
    assert!(
        engine
            .minds
            .get_mut(woman)
            .lexicon
            .produce(crate::Concept::Water)
            .is_some(),
        "and their own words"
    );

    // Not again for a few years.
    engine.family_events.clear();
    engine.time = SimTime::from_ticks(2 * TICKS_PER_YEAR);
    engine.age_people();
    assert!(engine.family_events().is_empty());
}
