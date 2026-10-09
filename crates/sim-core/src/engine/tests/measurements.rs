//! Release-only layout, scheduler, and footprint measurements for each physical-agent slice.

use super::*;

#[test]
#[ignore = "release-only Slice 0 layout and scheduler measurement"]
fn release_physical_agent_slice_zero_measurement() {
    assert!(
        !std::hint::black_box(cfg!(debug_assertions)),
        "run this measurement in release mode"
    );
    eprintln!(
        "population\tagent_size\tagent_align\tgeneration_size\tevent_size\tevent_align\trecord_capacity\tgeneration_capacity\tscheduler_capacity\toutcome_capacity\tretained_logical_bytes\tretained_buffers\tinsert_ns\tinsert_growth_allocations\treschedule_ns\treschedule_growth_allocations\tdue_extract_ns"
    );
    for population in [20_u32, 100, 10_000] {
        let mut engine = resident_engine(512);
        engine
            .initialize_population(
                PopulationInit {
                    active_area: engine.world().initial_bounds(),
                    population,
                },
                &[],
            )
            .unwrap();
        let capacities = engine.population.capacities();
        let record_capacity = capacities.records;
        let generation_capacity = capacities.movement_generations;
        let initial_scheduler_capacity = engine.scheduler.capacity();
        let outcome_capacity = engine.movement_outcomes.capacity();

        let repetitions = match population {
            20 => 10_000_u128,
            100 => 2_000,
            _ => 50,
        };
        let mut insert_ns = 0_u128;
        let mut reschedule_ns = 0_u128;
        let mut due_ns = 0_u128;
        let mut insert_growths = 0;
        let mut reschedule_growths = 0;
        for _ in 0..repetitions {
            let mut scheduler = Scheduler::with_capacity(population as usize);
            let before_insert_capacity = scheduler.capacity();
            let insert_start = Instant::now();
            for raw in 0..population {
                scheduler
                    .schedule_movement(
                        SimTime::from_ticks(10),
                        AgentId::new(raw),
                        1,
                        agent::CompactPosition { x: 0, y: 0 },
                    )
                    .unwrap();
            }
            insert_ns += insert_start.elapsed().as_nanos();
            insert_growths += usize::from(scheduler.capacity() != before_insert_capacity);
            std::hint::black_box(scheduler.len());

            let before_reschedule_capacity = scheduler.capacity();
            let reschedule_start = Instant::now();
            for raw in 0..population {
                scheduler
                    .schedule_movement(
                        SimTime::from_ticks(10),
                        AgentId::new(raw),
                        2,
                        agent::CompactPosition { x: 1, y: 0 },
                    )
                    .unwrap();
            }
            reschedule_ns += reschedule_start.elapsed().as_nanos();
            reschedule_growths += usize::from(scheduler.capacity() != before_reschedule_capacity);

            let due_start = Instant::now();
            let mut extracted = 0;
            while scheduler.pop_due(SimTime::from_ticks(10)).is_some() {
                extracted += 1;
            }
            due_ns += due_start.elapsed().as_nanos();
            assert_eq!(extracted, population as usize * 2);
        }
        insert_ns /= repetitions;
        reschedule_ns /= repetitions;
        due_ns /= repetitions;
        insert_growths /= repetitions as usize;
        reschedule_growths /= repetitions as usize;

        let retained_logical_bytes = record_capacity * size_of::<agent::AgentRecord>()
            + generation_capacity * size_of::<u32>()
            + initial_scheduler_capacity * size_of::<scheduler::ScheduledEvent>()
            + outcome_capacity * size_of::<MovementEventOutcome>();
        eprintln!(
            "{population}\t{}\t{}\t{}\t{}\t{}\t{record_capacity}\t{generation_capacity}\t{initial_scheduler_capacity}\t{outcome_capacity}\t{retained_logical_bytes}\t4\t{insert_ns}\t{insert_growths}\t{reschedule_ns}\t{reschedule_growths}\t{due_ns}",
            size_of::<agent::AgentRecord>(),
            std::mem::align_of::<agent::AgentRecord>(),
            size_of::<u32>(),
            size_of::<scheduler::ScheduledEvent>(),
            std::mem::align_of::<scheduler::ScheduledEvent>(),
        );
    }
}

#[test]
#[ignore = "release-only Slice 4 inventory and resource-delta measurement"]
fn release_physical_agent_slice_four_measurement() {
    assert!(
        !std::hint::black_box(cfg!(debug_assertions)),
        "run this measurement in release mode"
    );
    eprintln!(
        "population\tinventory_size\tinventory_align\tinventory_capacity\tinventory_logical_bytes"
    );
    for population in [20_u32, 100, 10_000] {
        let mut engine = resident_engine(512);
        engine
            .initialize_population(
                PopulationInit {
                    active_area: engine.world().initial_bounds(),
                    population,
                },
                &[],
            )
            .unwrap();
        let inventory_capacity = engine.population.inventory_capacity();
        eprintln!(
            "{population}\t{}\t{}\t{inventory_capacity}\t{}",
            size_of::<InventoryView>(),
            std::mem::align_of::<InventoryView>(),
            inventory_capacity * size_of::<InventoryView>(),
        );
    }

    let engine = resident_engine(256);
    let bounds = engine.world().initial_bounds();
    let position = (bounds.min.y..bounds.max.y)
        .flat_map(|y| (bounds.min.x..bounds.max.x).map(move |x| WorldPosition { x, y }))
        .find(|&position| {
            engine
                .world()
                .resource_at(position)
                .is_ok_and(|value| value.is_some())
        })
        .expect("measurement world should contain one generated resource");
    let base = engine.world().resource_at(position).unwrap().unwrap();
    let mut deltas = ResourceDeltas::default();
    let start = Instant::now();
    let mut gathered = 0_u16;
    while let Some((_, amount)) = deltas.gather(engine.world(), position, 1).unwrap() {
        gathered += u16::from(amount);
    }
    let gather_ns = start.elapsed().as_nanos();
    assert_eq!(gathered, base.capacity);
    assert_eq!(deltas.len(), 1);
    eprintln!(
        "resource_kind={:?}\tbase_capacity={}\tdelta_entry_size={}\tdelta_entry_align={}\tdelta_records={}\tgather_completions={}\tgather_total_ns={gather_ns}",
        base.kind,
        base.capacity,
        size_of::<resources::ResourceDelta>(),
        std::mem::align_of::<resources::ResourceDelta>(),
        deltas.len(),
        gathered,
    );
}

#[test]
#[ignore = "release-only Slice 5 sleep-state and wake-event measurement"]
fn release_physical_agent_slice_five_measurement() {
    assert!(
        !std::hint::black_box(cfg!(debug_assertions)),
        "run this measurement in release mode"
    );
    eprintln!(
        "population\tsleep_state_size\tsleep_state_align\tsleep_capacity\tsleep_logical_bytes\tscheduled_events\tschedule_ns\twake_extract_ns"
    );
    for population in [20_u32, 100, 10_000] {
        let mut engine = resident_engine(512);
        engine
            .initialize_population(
                PopulationInit {
                    active_area: engine.world().initial_bounds(),
                    population,
                },
                &[],
            )
            .unwrap();
        let positions: Vec<_> = engine
            .population
            .views(population as usize)
            .map(|view| view.position)
            .collect();
        let schedule_start = Instant::now();
        for (raw, position) in positions.into_iter().enumerate() {
            engine
                .population
                .schedule_sleep(
                    &mut engine.scheduler,
                    SimTime::ZERO,
                    AgentId::new(raw as u32),
                    position,
                    SleepQuality::OpenGround,
                    PolicyReason::RestThreshold,
                )
                .unwrap();
        }
        let schedule_ns = schedule_start.elapsed().as_nanos();
        let scheduled_events = engine.scheduler.len();
        let wake_start = Instant::now();
        let mut extracted = 0;
        while engine.scheduler.pop_due(SimTime::from_ticks(1)).is_some() {
            extracted += 1;
        }
        let wake_extract_ns = wake_start.elapsed().as_nanos();
        assert_eq!(extracted, population as usize);
        let sleep_capacity = engine.population.sleep_capacity();
        eprintln!(
            "{population}\t{}\t{}\t{sleep_capacity}\t{}\t{scheduled_events}\t{schedule_ns}\t{wake_extract_ns}",
            size_of::<sleep::SleepState>(),
            std::mem::align_of::<sleep::SleepState>(),
            sleep_capacity * size_of::<sleep::SleepState>(),
        );
    }
}

#[test]
#[ignore = "release-only Slice 1 spatial, perception, and route measurement"]
fn release_physical_agent_slice_one_measurement() {
    assert!(
        !std::hint::black_box(cfg!(debug_assertions)),
        "run this measurement in release mode"
    );
    eprintln!(
        "population\tspatial_entry_size\tspatial_entry_align\troute_state_size\tspatial_entry_capacity\tspatial_buckets\tspatial_logical_bytes\tperception_cells\tperceived_agents\twater\tresources\tperception_ns"
    );
    for population in [20_u32, 100, 10_000] {
        let mut engine = resident_engine(512);
        engine
            .initialize_population(
                PopulationInit {
                    active_area: engine.world().initial_bounds(),
                    population,
                },
                &[],
            )
            .unwrap();
        let capacities = engine.population.capacities();
        let route_capacity = capacities.routes;
        let spatial_capacity = capacities.occupancy_entry_capacity;
        let spatial_buckets = capacities.occupancy_buckets;
        let start = Instant::now();
        let perception = engine.perceive_physical(AgentId::new(0), 31).unwrap();
        let perception_ns = start.elapsed().as_nanos();
        let spatial_logical_bytes = spatial_capacity * size_of::<spatial::CellOccupant>()
            + route_capacity * size_of::<Option<agent::RouteState>>();
        let perception_cells = (perception.area.max.x - perception.area.min.x)
            * (perception.area.max.y - perception.area.min.y);
        eprintln!(
            "{population}\t{}\t{}\t{}\t{spatial_capacity}\t{spatial_buckets}\t{spatial_logical_bytes}\t{perception_cells}\t{}\t{}\t{}\t{perception_ns}",
            size_of::<spatial::CellOccupant>(),
            std::mem::align_of::<spatial::CellOccupant>(),
            size_of::<Option<agent::RouteState>>(),
            perception.agents.len(),
            perception.drinkable_water.len(),
            perception.resources.len(),
        );
    }

    let mut engine = resident_engine(512);
    let origin = standable_steps(&engine, 1)[0].0;
    engine
        .initialize_population(
            PopulationInit {
                active_area: engine.world().initial_bounds(),
                population: 1,
            },
            &[origin],
        )
        .unwrap();
    let active_area = engine.world().initial_bounds();
    let mut selected = None;
    'search: for radius in 8_i64..=48 {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx.abs().max(dy.abs()) != radius {
                    continue;
                }
                let destination = WorldPosition {
                    x: origin.x + dx,
                    y: origin.y + dy,
                };
                if !active_area.contains(destination) {
                    continue;
                }
                let request = RouteRequest {
                    destination,
                    max_expansions: MAX_ROUTE_EXPANSIONS,
                };
                if let Ok(plan) = engine.route_planner.plan(
                    RouteEnvironment {
                        world: &engine.world,
                        spawned_objects: &engine.spawned_objects,
                        structures: &engine.structures,
                        active_area,
                    },
                    origin,
                    request,
                ) && plan.expansions >= 64
                {
                    selected = Some((request, plan.expansions));
                    break 'search;
                }
            }
        }
    }
    let (request, expansions) = selected.expect("seeded area should have a measured local route");
    let capacities_before = engine.route_planner.capacities();
    let repetitions = 1_000_u128;
    let start = Instant::now();
    for _ in 0..repetitions {
        let plan = engine
            .route_planner
            .plan(
                RouteEnvironment {
                    world: &engine.world,
                    spawned_objects: &engine.spawned_objects,
                    structures: &engine.structures,
                    active_area,
                },
                origin,
                request,
            )
            .unwrap();
        assert_eq!(plan.expansions, expansions);
        std::hint::black_box(plan.next);
    }
    let average_ns = start.elapsed().as_nanos() / repetitions;
    let capacities_after = engine.route_planner.capacities();
    eprintln!(
        "route_expansions\t{expansions}\troute_average_ns\t{average_ns}\tnode_capacity\t{}\tlookup_capacity\t{}\topen_capacity\t{}\tgrowth_buffers\t{}",
        capacities_after.0,
        capacities_after.1,
        capacities_after.2,
        usize::from(capacities_before.0 != capacities_after.0)
            + usize::from(capacities_before.1 != capacities_after.1)
            + usize::from(capacities_before.2 != capacities_after.2),
    );
}

#[test]
#[ignore = "release-only Slice 2 analytical-needs measurement"]
fn release_physical_agent_slice_two_measurement() {
    assert!(
        !std::hint::black_box(cfg!(debug_assertions)),
        "run this measurement in release mode"
    );
    eprintln!(
        "population\tneed_state_size\tneed_state_align\tthreshold_event_size\tneed_capacity\tscheduler_capacity\tinitial_events\trescheduled_events\tschedule_ns\tdue_extract_ns\tgrowth_buffers\tretained_logical_bytes"
    );
    for population in [20_usize, 100, 10_000] {
        let mut states = Vec::with_capacity(population);
        states.resize(population, needs::NeedState::new(SimTime::ZERO));
        let mut scheduler = Scheduler::with_capacity(population * 4);
        for (raw, state) in states.iter().copied().enumerate() {
            for kind in NeedKind::ALL {
                if let Some(due) = state.threshold_due(kind, SimTime::ZERO) {
                    scheduler
                        .schedule_need_threshold(due, AgentId::new(raw as u32), 0, kind)
                        .unwrap();
                }
            }
        }
        let initial_events = scheduler.len();
        let before_capacity = scheduler.capacity();
        let schedule_start = Instant::now();
        let mut rescheduled_events = 0;
        for (raw, state) in states.iter_mut().enumerate() {
            state.transition(AgentActivity::Moving, SimTime::from_ticks(1));
            for kind in NeedKind::ALL {
                if let Some(due) = state.threshold_due(kind, SimTime::from_ticks(1)) {
                    scheduler
                        .schedule_need_threshold(
                            due,
                            AgentId::new(raw as u32),
                            state.generation(),
                            kind,
                        )
                        .unwrap();
                    rescheduled_events += 1;
                }
            }
        }
        let schedule_ns = schedule_start.elapsed().as_nanos();
        let growth_buffers = usize::from(scheduler.capacity() != before_capacity);
        let due_start = Instant::now();
        let mut extracted = 0;
        while scheduler.pop_due(SimTime::from_ticks(u64::MAX)).is_some() {
            extracted += 1;
        }
        let due_extract_ns = due_start.elapsed().as_nanos();
        assert_eq!(extracted, initial_events + rescheduled_events);
        let retained_logical_bytes = states.capacity() * size_of::<needs::NeedState>()
            + scheduler.capacity() * size_of::<scheduler::ScheduledEvent>();
        eprintln!(
            "{population}\t{}\t{}\t{}\t{}\t{}\t{initial_events}\t{rescheduled_events}\t{schedule_ns}\t{due_extract_ns}\t{growth_buffers}\t{retained_logical_bytes}",
            size_of::<needs::NeedState>(),
            std::mem::align_of::<needs::NeedState>(),
            size_of::<scheduler::ScheduledEvent>(),
            states.capacity(),
            scheduler.capacity(),
        );
    }
}

#[test]
#[ignore = "release-only Slice 3 physical-policy measurement"]
fn release_physical_agent_slice_three_measurement() {
    assert!(
        !std::hint::black_box(cfg!(debug_assertions)),
        "run this measurement in release mode"
    );
    eprintln!(
        "population\tpolicy_state_size\tpolicy_state_align\tdecision_event_size\tpolicy_capacity\tscheduler_capacity\tschedule_ns\tdue_extract_ns\tgrowth_buffers\tretained_logical_bytes"
    );
    for population in [20_usize, 100, 10_000] {
        let mut states = Vec::with_capacity(population);
        states.resize(population, policy::PolicyState::default());
        let mut scheduler = Scheduler::with_capacity(population);
        let before_capacity = scheduler.capacity();
        let schedule_start = Instant::now();
        for (raw, state) in states.iter_mut().enumerate() {
            let generation = state.next_generation().unwrap();
            state.set_phase(policy::PolicyPhase::DecisionPending);
            scheduler
                .schedule_decision(
                    SimTime::from_ticks(1),
                    AgentId::new(raw as u32),
                    generation,
                    PhysicalGoal::Wait,
                )
                .unwrap();
        }
        let schedule_ns = schedule_start.elapsed().as_nanos();
        let growth_buffers = usize::from(scheduler.capacity() != before_capacity);
        let due_start = Instant::now();
        let mut extracted = 0;
        while scheduler.pop_due(SimTime::from_ticks(1)).is_some() {
            extracted += 1;
        }
        let due_extract_ns = due_start.elapsed().as_nanos();
        assert_eq!(extracted, population);
        let retained_logical_bytes = states.capacity() * size_of::<policy::PolicyState>()
            + scheduler.capacity() * size_of::<scheduler::ScheduledEvent>();
        eprintln!(
            "{population}\t{}\t{}\t{}\t{}\t{}\t{schedule_ns}\t{due_extract_ns}\t{growth_buffers}\t{retained_logical_bytes}",
            size_of::<policy::PolicyState>(),
            std::mem::align_of::<policy::PolicyState>(),
            size_of::<scheduler::ScheduledEvent>(),
            states.capacity(),
            scheduler.capacity(),
        );
    }
}
