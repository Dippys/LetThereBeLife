//! Shelter construction, interruption, reservation, and contention tests.

use super::*;

#[test]
fn shelter_build_blocks_travel_completes_and_enables_safer_sleep() {
    let mut engine = resident_engine(64);
    let (access, site) = standable_shelter_site(&engine);
    engine
        .initialize_population(
            PopulationInit {
                active_area: engine.world().initial_bounds(),
                population: 1,
            },
            &[access],
        )
        .unwrap();
    engine
        .population
        .add_inventory(AgentId::new(0), crate::Material::Wood, SHELTER_WOOD_COST);
    engine
        .population
        .add_inventory(AgentId::new(0), crate::Material::Stone, SHELTER_STONE_COST);

    let started = engine.request_build_shelter(AgentId::new(0), site).unwrap();
    assert_eq!(started.state, StructureState::UnderConstruction);
    assert_eq!(engine.snapshot().structure_count, 1);
    assert_eq!(
        engine
            .inventory(AgentId::new(0))
            .unwrap()
            .amount(crate::Material::Wood),
        0
    );
    assert_eq!(
        engine
            .inventory(AgentId::new(0))
            .unwrap()
            .amount(crate::Material::Stone),
        0
    );
    let perception = engine.perceive_physical(AgentId::new(0), 2).unwrap();
    assert_eq!(perception.structures, [started]);
    assert!(!perception.traversable_cells.contains(&site));
    assert_eq!(
        engine.request_move(AgentId::new(0), site),
        Err(MoveRequestError::BlockedByStructure(started.id))
    );
    assert_eq!(
        engine.request_route(
            AgentId::new(0),
            RouteRequest {
                destination: site,
                max_expansions: 16,
            },
        ),
        Err(RouteRequestError::BlockedByStructure(started.id))
    );

    while engine.snapshot().tick < started.completes_at.ticks() {
        engine.tick();
    }
    let completed = engine.structure_views(1).next().unwrap();
    assert_eq!(completed.state, StructureState::Complete);
    assert_eq!(completed.builder, None);
    assert_eq!(engine.structure_diagnostics().len(), 1);
    assert_eq!(
        engine.structure_diagnostics()[0].kind,
        StructureDiagnosticKind::Completed
    );

    let exposure_before = engine
        .physical_needs(AgentId::new(0))
        .unwrap()
        .exposure
        .value;
    let sleep = engine.request_sleep(AgentId::new(0), access).unwrap();
    assert_eq!(sleep.quality, SleepQuality::Sheltered);
    for _ in 0..60 {
        engine.tick();
    }
    assert!(
        engine
            .physical_needs(AgentId::new(0))
            .unwrap()
            .exposure
            .value
            < exposure_before
    );
}

#[test]
fn construction_interruption_refunds_once_and_stale_completion_is_harmless() {
    let mut engine = resident_engine(64);
    let (access, site) = standable_shelter_site(&engine);
    engine
        .initialize_population(
            PopulationInit {
                active_area: engine.world().initial_bounds(),
                population: 1,
            },
            &[access],
        )
        .unwrap();
    engine
        .population
        .add_inventory(AgentId::new(0), crate::Material::Wood, SHELTER_WOOD_COST);
    engine
        .population
        .add_inventory(AgentId::new(0), crate::Material::Stone, SHELTER_STONE_COST);
    engine.time = SimTime::from_ticks(89_950);
    let started = engine
        .start_build(
            AgentId::new(0),
            site,
            PolicyReason::NoUrgentNeed,
            crate::StructureKind::Shelter,
        )
        .unwrap();

    while engine.snapshot().structure_count != 0 {
        engine.tick();
    }
    let cancelled_at = engine.snapshot().tick;
    assert_eq!(engine.snapshot().structure_count, 0);
    assert_eq!(
        engine
            .inventory(AgentId::new(0))
            .unwrap()
            .amount(crate::Material::Wood),
        SHELTER_WOOD_COST
    );
    assert_eq!(
        engine
            .inventory(AgentId::new(0))
            .unwrap()
            .amount(crate::Material::Stone),
        SHELTER_STONE_COST
    );
    assert_eq!(
        engine.structure_diagnostics()[0].kind,
        StructureDiagnosticKind::Cancelled
    );
    assert_eq!(
        engine.agent_views(1).next().unwrap().activity,
        AgentActivity::Idle
    );
    // A long build: keep the builder alive while the stale completion comes due.
    while engine.snapshot().tick <= started.completes_at.ticks().max(cancelled_at) {
        for need in [
            crate::NeedKind::Hunger,
            crate::NeedKind::Thirst,
            crate::NeedKind::Rest,
            crate::NeedKind::Exposure,
        ] {
            engine
                .population
                .set_need_value_for_test(AgentId::new(0), need, 0, engine.time);
        }
        engine.tick();
    }
    assert_eq!(engine.snapshot().structure_count, 0);
    assert_eq!(
        engine
            .inventory(AgentId::new(0))
            .unwrap()
            .amount(crate::Material::Wood),
        SHELTER_WOOD_COST
    );
}

#[test]
fn structure_reserved_after_move_request_blocks_at_movement_completion() {
    let mut engine = resident_engine(128);
    let bounds = engine.world().initial_bounds();
    let mut found = None;
    'rows: for y in bounds.min.y + 1..bounds.max.y - 1 {
        for x in bounds.min.x + 1..bounds.max.x - 1 {
            let site = WorldPosition { x, y };
            let from = WorldPosition { x: x - 1, y };
            let builder = WorldPosition { x: x + 1, y };
            if [from, site, builder].into_iter().all(|position| {
                engine.world().standability_at(position) == Ok(Standability::Standable)
            }) {
                found = Some((from, site, builder));
                break 'rows;
            }
        }
    }
    let (from, site, builder) =
        found.expect("seeded world should contain three horizontal standable cells");
    engine
        .initialize_population(
            PopulationInit {
                active_area: bounds,
                population: 2,
            },
            &[from, builder],
        )
        .unwrap();
    engine
        .population
        .add_inventory(AgentId::new(1), crate::Material::Wood, SHELTER_WOOD_COST);
    let movement = engine.request_move(AgentId::new(0), site).unwrap();
    let shelter = engine.request_build_shelter(AgentId::new(1), site).unwrap();
    while engine.snapshot().tick < movement.completes_at.ticks() {
        engine.tick();
    }
    assert_eq!(
        engine.movement_outcomes()[0].kind,
        MovementOutcomeKind::BlockedByStructure(shelter.id)
    );
    assert_eq!(engine.agent_views(1).next().unwrap().position, from);
}

#[test]
fn equal_time_builders_resolve_overlap_by_agent_id_without_double_spending() {
    let mut engine = resident_engine(128);
    let bounds = engine.world().initial_bounds();
    let center = (bounds.min.y + 2..bounds.max.y - 2)
        .flat_map(|y| (bounds.min.x + 2..bounds.max.x - 2).map(move |x| WorldPosition { x, y }))
        .find(|center| {
            (-2..=2).all(|dx| {
                (-1..=1).all(|dy| {
                    engine.world().standability_at(WorldPosition {
                        x: center.x + dx,
                        y: center.y + dy,
                    }) == Ok(Standability::Standable)
                })
            })
        })
        .expect("seeded world should contain a standable 5x3 construction test patch");
    let positions = [
        WorldPosition {
            x: center.x - 1,
            y: center.y,
        },
        WorldPosition {
            x: center.x + 1,
            y: center.y,
        },
        WorldPosition {
            x: center.x - 2,
            y: center.y,
        },
        WorldPosition {
            x: center.x - 1,
            y: center.y - 1,
        },
        WorldPosition {
            x: center.x - 1,
            y: center.y + 1,
        },
        WorldPosition {
            x: center.x + 2,
            y: center.y,
        },
        WorldPosition {
            x: center.x + 1,
            y: center.y - 1,
        },
        WorldPosition {
            x: center.x + 1,
            y: center.y + 1,
        },
    ];
    engine
        .initialize_population(
            PopulationInit {
                active_area: bounds,
                population: positions.len() as u32,
            },
            &positions,
        )
        .unwrap();
    for agent in [AgentId::new(0), AgentId::new(1)] {
        engine
            .population
            .add_inventory(agent, crate::Material::Wood, SHELTER_WOOD_COST);
        engine
            .population
            .add_inventory(agent, crate::Material::Stone, SHELTER_STONE_COST);
    }
    engine.activate_physical_policy().unwrap();
    engine.tick();

    let structure = engine.structure_views(1).next().unwrap();
    assert_eq!(structure.position, center);
    assert_eq!(structure.builder, Some(AgentId::new(0)));
    assert_eq!(
        engine
            .inventory(AgentId::new(0))
            .unwrap()
            .amount(crate::Material::Wood),
        0
    );
    assert_eq!(
        engine
            .inventory(AgentId::new(1))
            .unwrap()
            .amount(crate::Material::Wood),
        SHELTER_WOOD_COST
    );
}

#[test]
#[ignore = "release-only Slice 6 structure layout and scheduler concentration measurement"]
fn release_slice6_structure_measurement() {
    for count in [20_u32, 100, 10_000] {
        let mut store = StructureStore::default();
        let mut scheduler = Scheduler::with_capacity(count as usize);
        let build_start = Instant::now();
        for raw in 0..count {
            store
                .start(
                    AgentId::new(raw),
                    WorldPosition {
                        x: i64::from(raw % 200),
                        y: i64::from(raw / 200),
                    },
                    crate::StructureKind::Shelter,
                    SimTime::ZERO,
                    SimTime::from_ticks(SHELTER_BUILD_TICKS),
                )
                .unwrap();
            scheduler
                .schedule_action_completion(
                    SimTime::from_ticks(SHELTER_BUILD_TICKS),
                    AgentId::new(raw),
                    1,
                    PhysicalGoal::BuildShelter,
                    agent::CompactPosition {
                        x: (raw % 200) as i16,
                        y: (raw / 200) as i16,
                    },
                )
                .unwrap();
        }
        let build_ns = build_start.elapsed().as_nanos();
        let extraction_start = Instant::now();
        while scheduler
            .pop_due(SimTime::from_ticks(SHELTER_BUILD_TICKS))
            .is_some()
        {}
        println!(
            "slice6 structures={count} record_bytes={} slot_bytes={} index_entry_bytes={} event_bytes={} retained_slots={} logical_record_bytes={} build_schedule_ns={} due_extract_ns={}",
            size_of::<structures::StructureRecord>(),
            size_of::<Option<structures::StructureRecord>>(),
            size_of::<((i16, i16), StructureId)>(),
            size_of::<scheduler::ScheduledEvent>(),
            store.retained_slots(),
            store.retained_slots() * size_of::<Option<structures::StructureRecord>>(),
            build_ns,
            extraction_start.elapsed().as_nanos(),
        );
    }
}
