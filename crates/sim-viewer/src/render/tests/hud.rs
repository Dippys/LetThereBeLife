//! Overlay geometry, HUD text, instance-budget, and agent-panel tests.

use bytemuck::Zeroable;
use sim_core::{
    ACQUAINTANCE_SLOTS, AcquaintanceView, AgentActivity, AgentView, DEFAULT_TRUST, DeathCause,
    DeathRecord, ExplorationHeading, HealthStatus, HealthView, InventoryView, LANDMARK_SLOTS,
    LandmarkKind, LandmarkSource, LandmarkView, MentalMapView, NeedKind, Personality, PhysicalGoal,
    PhysicalNeedsView, PhysicalPolicyView, PolicyReason, SleepQuality, SleepView, SpawnKind,
    VISITED_TILE_SLOTS, WORLD_GENERATION_BOUNDS, World, WorldConfig, WorldPosition,
};

use super::test_render_state;
use crate::render::colors::{rgba, selection_color};
use crate::render::gpu::{Instance, static_instance_chunks};
use crate::render::hud::{
    AGENT_CARD_LINE_WIDTH, personality_summary, write_agent_text, write_hud_text,
};
use crate::render::instances::{chunk_outline, world_border};
use crate::render::overlay::build_screen_overlay;
use crate::render::{
    AGENT_TEXT_CAPACITY, AgentInspection, GenerationStatus, HUD_TEXT_CAPACITY,
    MAX_INSTANCES_PER_BUFFER, MemoryInspection, SCREEN_OVERLAY_CAPACITY, SpawnMenuView,
};

fn landmark(kind: LandmarkKind, source: LandmarkSource, x: i64) -> LandmarkView {
    LandmarkView {
        kind,
        position: WorldPosition { x, y: 0 },
        source,
        confidence: 200,
        search_radius: 0,
        seen_second: 10,
    }
}

fn acquaintance(id: u32, familiarity: u8, trust: u8) -> AcquaintanceView {
    AcquaintanceView {
        agent: sim_core::AgentId::new(id),
        familiarity,
        trust,
        last_seen_position: Some(WorldPosition { x: 3, y: 4 }),
        last_seen_second: 20,
    }
}

fn mind(personality: Personality, acquaintances: Vec<AcquaintanceView>) -> MemoryInspection {
    MemoryInspection::from_view(&MentalMapView {
        agent: sim_core::AgentId::new(1),
        personality,
        landmarks: Vec::new(),
        explored_tiles: 0,
        acquaintances,
    })
}

fn card_lines(memory: MemoryInspection) -> Vec<String> {
    let inspection = AgentInspection {
        view: AgentView {
            id: sim_core::AgentId::new(1),
            position: WorldPosition { x: 0, y: 0 },
            activity: AgentActivity::Idle,
        },
        needs: None,
        inventory: None,
        health: None,
        policy: None,
        sleep: None,
        death: None,
        memory: Some(memory),
    };
    let mut text = String::new();
    write_agent_text(&mut text, Some(inspection));
    text.lines()
        .skip_while(|line| !line.starts_with("SHELTER "))
        .skip(1)
        .map(str::to_owned)
        .collect()
}

#[test]
fn personality_summary_names_the_most_pronounced_trait_deterministically() {
    let traits = |curiosity, caution, sociability, diligence| Personality {
        curiosity,
        caution,
        sociability,
        diligence,
    };
    assert_eq!(personality_summary(Personality::AVERAGE), "BALANCED");
    assert_eq!(personality_summary(traits(175, 81, 128, 128)), "BALANCED");
    assert_eq!(personality_summary(traits(220, 128, 128, 128)), "EXPLORER");
    assert_eq!(personality_summary(traits(20, 128, 128, 128)), "HOMEBODY");
    assert_eq!(personality_summary(traits(128, 200, 128, 128)), "CAREFUL");
    assert_eq!(personality_summary(traits(128, 30, 128, 128)), "DARING");
    assert_eq!(personality_summary(traits(150, 128, 240, 128)), "SOCIABLE");
    assert_eq!(personality_summary(traits(150, 128, 10, 200)), "LONER");
    assert_eq!(personality_summary(traits(128, 128, 128, 250)), "DILIGENT");
    assert_eq!(personality_summary(traits(128, 128, 128, 0)), "EASYGOING");
    // Equal deviations go to the earlier trait in field order.
    assert_eq!(personality_summary(traits(200, 56, 200, 56)), "EXPLORER");
    assert_eq!(personality_summary(traits(128, 56, 200, 128)), "DARING");
}

#[test]
fn agent_card_lists_personality_and_most_familiar_friends() {
    let personality = Personality {
        curiosity: 183,
        caution: 63,
        sociability: 227,
        diligence: 138,
    };
    let lines = card_lines(mind(
        personality,
        vec![
            acquaintance(9, FRIEND - 1, 250),
            acquaintance(12, 40, 160),
            acquaintance(7, 40, DEFAULT_TRUST),
            acquaintance(3, FRIEND, DEFAULT_TRUST),
            acquaintance(30, 60, 90),
        ],
    ));
    assert_eq!(
        lines,
        [
            "SOCIABLE  CUR 183 CAU 63 SOC 227 DIL 138",
            "FRIENDS 4 OF 5 KNOWN",
            "TOP  #30 T90  #12 T160  #7 T128  #3 T128",
        ]
    );

    let lines = card_lines(mind(Personality::AVERAGE, Vec::new()));
    assert_eq!(
        lines,
        [
            "BALANCED  CUR 128 CAU 128 SOC 128 DIL 128",
            "FRIENDS 0 OF 0 KNOWN"
        ]
    );
    let lines = card_lines(mind(
        Personality::AVERAGE,
        vec![acquaintance(5, FRIEND - 1, DEFAULT_TRUST)],
    ));
    assert_eq!(lines[1..], ["FRIENDS 0 OF 1 KNOWN"]);

    // Long ids are dropped from the TOP line rather than widening the card.
    let lines = card_lines(mind(
        Personality::AVERAGE,
        vec![acquaintance(u32::MAX, u8::MAX, u8::MAX); ACQUAINTANCE_SLOTS],
    ));
    assert_eq!(lines[1], "FRIENDS 6 OF 6 KNOWN");
    assert_eq!(lines[2], "TOP  #4294967295 T255  #4294967295 T255");
    assert!(lines.iter().all(|line| line.len() <= AGENT_CARD_LINE_WIDTH));
}

const FRIEND: u8 = sim_core::FRIEND_FAMILIARITY;

#[test]
fn static_buffers_partition_at_the_device_safe_limit() {
    let instances = vec![Instance::zeroed(); MAX_INSTANCES_PER_BUFFER + 1];
    let lengths: Vec<_> = static_instance_chunks(&instances).map(<[_]>::len).collect();
    assert_eq!(lengths, [MAX_INSTANCES_PER_BUFFER, 1]);
}

#[test]
fn invalid_selection_uses_red_preview() {
    assert_eq!(selection_color(true), rgba(255, 220, 35, 72));
    assert_eq!(selection_color(false), rgba(235, 48, 48, 96));
}

#[test]
fn world_border_marks_the_centered_generation_envelope() {
    let border = world_border(1.0);

    assert_eq!(border[0].position, [-32_768.0, -32_768.0]);
    assert_eq!(border[0].size, [65_536.0, 2.0]);
    assert_eq!(border[1].position, [-32_768.0, 32_766.0]);
    assert_eq!(border[2].size, [2.0, 65_536.0]);
    assert_eq!(border[3].position, [32_766.0, -32_768.0]);
    assert!(
        border
            .iter()
            .all(|instance| instance.color == rgba(245, 40, 40, 230))
    );
}

#[test]
fn chunk_outline_uses_signed_chunk_bounds() {
    let world = World::generate(1, WorldConfig::new(64, 64).unwrap());
    let inspection = world
        .inspect_chunk_at(WorldPosition { x: -1, y: 63 })
        .unwrap();
    let outline = chunk_outline(inspection, 1.0).unwrap();

    assert_eq!(outline[0].position, [-64.0, 0.0]);
    assert_eq!(outline[0].size, [64.0, 1.0]);
    assert_eq!(outline[1].position, [-64.0, 63.0]);
    assert_eq!(outline[2].position, [-64.0, 0.0]);
    assert_eq!(outline[3].position, [-1.0, 0.0]);
}

#[test]
fn subpixel_chunks_do_not_create_inspection_overlays() {
    let world = World::generate(1, WorldConfig::new(64, 64).unwrap());
    let inspection = world
        .inspect_chunk_at(WorldPosition { x: 0, y: 0 })
        .unwrap();
    assert!(chunk_outline(inspection, 0.01).is_none());
    assert!(chunk_outline(inspection, 0.1).is_some());
}

#[test]
fn hud_contains_simulation_and_loaded_cell_inspection_data() {
    let world = World::generate(7, WorldConfig::new(64, 64).unwrap());
    let state = test_render_state(Some(WorldPosition { x: 0, y: 0 }));
    let mut text = String::new();

    write_hud_text(&mut text, &world, &state);

    assert!(text.contains("RUNNING  SPEED 4X"));
    assert!(text.contains("SIM 0000:01:02.0  TICK 3721"));
    assert!(text.contains("SEED 7  LOADED "));
    assert!(text.contains("AGENTS ACTIVE  TOTAL 0  LIVING 0  ACTIVE 0  DEAD 0"));
    assert!(text.contains("  REV "));
    assert!(text.contains("CURSOR X 0  Y 0"));
    assert!(text.contains("CHUNK X 0 Y 0  LOCAL 0,0"));
    assert!(text.contains("COVERAGE "));
    assert!(text.contains("SURFACE "));
    assert!(text.contains("  BIOME "));
    assert!(text.contains("ELEV "));
    assert!(text.contains("TEMP "));
    assert!(text.contains("MOIST "));
    assert!(text.contains("WIND "));
    assert!(text.contains("FEATURE "));
}

#[test]
fn hud_reports_unloaded_coverage_and_worker_failure_in_game() {
    let world = World::new(7, WorldConfig::new(96, 64).unwrap());
    let mut state = test_render_state(Some(WorldPosition { x: 47, y: 0 }));
    state.generation_status = GenerationStatus::WorkerUnavailable;
    let mut text = String::new();

    write_hud_text(&mut text, &world, &state);

    assert!(text.contains("GEN WORKER OFFLINE"));
    assert!(text.contains("COVERAGE PARTIAL INITIAL UNLOADED"));
    assert!(text.contains("CELL UNLOADED"));
}

#[test]
fn every_hud_layout_fits_the_fixed_gpu_instance_budget() {
    let world = World::generate(u64::MAX, WorldConfig::new(64, 64).unwrap());
    let mut state = test_render_state(None);
    state.snapshot.tick = u64::MAX;
    state.snapshot.simulated_seconds = u64::MAX as f64 / 60.0;
    state.snapshot.seed = u64::MAX;
    state.snapshot.speed = 64.0;
    state.selection = Some(WORLD_GENERATION_BOUNDS);
    state.selection_valid = false;
    state.spawn_menu = Some(SpawnMenuView {
        selected: SpawnKind::BerryBush,
        placing: true,
    });
    let mut text = String::new();
    let mut instances = Vec::new();

    for cursor in [
        None,
        Some(WorldPosition { x: 0, y: 0 }),
        Some(WorldPosition {
            x: 32_768,
            y: 32_768,
        }),
    ] {
        state.cursor_world = cursor;
        for status in [
            GenerationStatus::Idle,
            GenerationStatus::Bootstrap,
            GenerationStatus::Manual,
            GenerationStatus::Cancelling,
            GenerationStatus::WorkerUnavailable,
        ] {
            state.generation_status = status;
            write_hud_text(&mut text, &world, &state);
            assert!(text.len() <= HUD_TEXT_CAPACITY);
            build_screen_overlay(&mut instances, &text, None, &state, 1_920, 1_080);
            assert!(instances.len() <= SCREEN_OVERLAY_CAPACITY);
        }
    }
}

#[test]
fn hovered_agent_panel_reports_authoritative_physical_state() {
    let view = AgentView {
        id: sim_core::AgentId::new(7),
        position: WorldPosition { x: 12, y: -9 },
        activity: AgentActivity::Moving,
    };
    let level = sim_core::NeedLevelView {
        value: 1_234,
        rate_per_period: 6,
        threshold: 6_000,
        threshold_reached: false,
    };
    let inspection = AgentInspection {
        view,
        needs: Some(PhysicalNeedsView {
            agent: view.id,
            at: sim_core::SimTime::from_ticks(90),
            hunger: level,
            thirst: level,
            rest: level,
            exposure: level,
            next_threshold: Some(sim_core::NeedThreshold {
                kind: NeedKind::Thirst,
                due: sim_core::SimTime::from_ticks(1_000),
            }),
        }),
        inventory: Some(InventoryView {
            food: 2,
            wood: 3,
            stone: 4,
        }),
        health: Some(HealthView {
            agent: view.id,
            value: 9_000,
            status: HealthStatus::Healthy,
            next_consequence: None,
        }),
        policy: Some(PhysicalPolicyView {
            agent: view.id,
            goal: PhysicalGoal::Explore,
            reason: PolicyReason::NoUrgentNeed,
            target: Some(WorldPosition { x: 20, y: -4 }),
            committed: true,
            retry_count: 1,
            exploration_heading: ExplorationHeading::NorthEast,
        }),
        sleep: None,
        death: None,
        memory: Some(MemoryInspection::from_view(&MentalMapView {
            personality: Personality::AVERAGE,
            acquaintances: Vec::new(),
            agent: view.id,
            landmarks: vec![
                landmark(LandmarkKind::Water, LandmarkSource::Seen, 0),
                landmark(LandmarkKind::Water, LandmarkSource::Told, 1),
                landmark(LandmarkKind::Food, LandmarkSource::Seen, 2),
                landmark(LandmarkKind::Shelter, LandmarkSource::Told, 3),
            ],
            explored_tiles: 37,
        })),
    };
    let mut text = String::with_capacity(AGENT_TEXT_CAPACITY);
    write_agent_text(&mut text, Some(inspection));
    assert!(text.contains("AGENT 7"));
    assert!(text.contains("ACTIVITY MOVING"));
    assert!(text.contains("GOAL EXPLORE"));
    assert!(text.contains("WHY NO URGENT NEED"));
    assert!(text.contains("STATUS COMMITTED  RETRIES 1"));
    assert!(text.contains("THIRST 1234 OF 6000  RATE +6"));
    assert!(text.contains("INVENTORY F 2  W 3  S 4"));
    assert!(text.contains("HEALTH 9000  HEALTHY"));
    assert!(text.contains("SLEEP NONE"));
    assert!(text.contains("MEMORY WATER 2  FOOD 1  WOOD 0  STONE 0\n"));
    assert!(text.contains("SHELTER 1  HINTS 2  EXPLORED 37 TILES\n"));
    assert!(text.contains("BALANCED  CUR 128 CAU 128 SOC 128 DIL 128\n"));
    assert!(text.contains("FRIENDS 0 OF 0 KNOWN\n"));
    assert!(text.len() <= AGENT_TEXT_CAPACITY);

    let mut mindless = inspection;
    mindless.memory = None;
    write_agent_text(&mut text, Some(mindless));
    assert!(text.contains("MEMORY NONE"));

    let mut backoff = inspection;
    backoff.policy = Some(PhysicalPolicyView {
        goal: PhysicalGoal::SeekWater,
        reason: PolicyReason::Retry,
        target: None,
        committed: false,
        retry_count: u8::MAX,
        ..backoff.policy.unwrap()
    });
    write_agent_text(&mut text, Some(backoff));
    assert!(text.contains("GOAL SEEK WATER"));
    assert!(text.contains("WHY RETRY"));
    assert!(text.contains("STATUS BACKOFF  RETRIES 255"));

    let budget_view = AgentView {
        id: sim_core::AgentId::new(u32::MAX),
        position: WorldPosition {
            x: -32_768,
            y: 32_767,
        },
        activity: AgentActivity::Incapacitated,
    };
    let budget_level = sim_core::NeedLevelView {
        value: 10_000,
        rate_per_period: -12,
        threshold: 10_000,
        threshold_reached: true,
    };
    let budget_inspection = AgentInspection {
        view: budget_view,
        needs: Some(PhysicalNeedsView {
            agent: budget_view.id,
            at: sim_core::SimTime::from_ticks(u64::MAX),
            hunger: budget_level,
            thirst: budget_level,
            rest: budget_level,
            exposure: budget_level,
            next_threshold: Some(sim_core::NeedThreshold {
                kind: NeedKind::Exposure,
                due: sim_core::SimTime::from_ticks(u64::MAX),
            }),
        }),
        inventory: Some(InventoryView {
            food: u8::MAX,
            wood: u8::MAX,
            stone: u8::MAX,
        }),
        health: Some(HealthView {
            agent: budget_view.id,
            value: 10_000,
            status: HealthStatus::Incapacitated,
            next_consequence: Some(sim_core::SimTime::from_ticks(u64::MAX)),
        }),
        policy: Some(PhysicalPolicyView {
            agent: budget_view.id,
            goal: PhysicalGoal::Incapacitated,
            reason: PolicyReason::ExposureThreshold,
            target: Some(WorldPosition {
                x: -32_768,
                y: 32_767,
            }),
            committed: true,
            retry_count: u8::MAX,
            exploration_heading: ExplorationHeading::SouthWest,
        }),
        sleep: Some(SleepView {
            agent: budget_view.id,
            position: budget_view.position,
            started_at: sim_core::SimTime::from_ticks(u64::MAX),
            planned_wake: sim_core::SimTime::from_ticks(u64::MAX),
            quality: SleepQuality::OpenGround,
        }),
        death: Some(DeathRecord {
            agent: budget_view.id,
            cause: DeathCause::Exhaustion,
            at: sim_core::SimTime::from_ticks(u64::MAX),
            position: budget_view.position,
        }),
        memory: Some(MemoryInspection::from_view(&MentalMapView {
            // Glyph instances are per lit pixel run, so the worst case maximizes
            // runs, not characters: '0' has the most runs of any digit, and four
            // 3-digit traits are only possible while BALANCED. That makes this the
            // heaviest personality line (41 characters).
            personality: Personality {
                curiosity: 100,
                caution: 100,
                sociability: 100,
                diligence: 100,
            },
            // Every slot a friend; ten-digit ids and 3-digit trust heavy in '0's.
            // The TOP line fits two of them (39 characters), and fewer, longer
            // entries out-weigh more, shorter ones.
            acquaintances: vec![acquaintance(4_000_000_000, u8::MAX, 100); ACQUAINTANCE_SLOTS],
            agent: budget_view.id,
            landmarks: (0..LANDMARK_SLOTS)
                .map(|index| landmark(LandmarkKind::Shelter, LandmarkSource::Told, index as i64))
                .collect(),
            // The mental map caps explored tiles at its fixed visit-tile slots.
            explored_tiles: VISITED_TILE_SLOTS,
        })),
    };
    let mut budget_text = String::with_capacity(AGENT_TEXT_CAPACITY);
    write_agent_text(&mut budget_text, Some(budget_inspection));
    assert!(budget_text.contains("WHY EXPOSURE THRESHOLD"));
    assert!(budget_text.contains("DEATH CAUSE EXHAUSTION"));
    assert!(budget_text.contains("DIED AT TICK 18446744073709551615"));
    assert!(budget_text.contains("SHELTER 12  HINTS 12  EXPLORED 24 TILES"));
    assert!(budget_text.contains("BALANCED  CUR 100 CAU 100 SOC 100 DIL 100\n"));
    assert!(budget_text.contains("FRIENDS 6 OF 6 KNOWN\n"));
    assert!(budget_text.contains("TOP  #4000000000 T100  #4000000000 T100\n"));
    assert!(budget_text.len() <= AGENT_TEXT_CAPACITY);

    let world = World::generate(u64::MAX, WorldConfig::new(64, 64).unwrap());
    let mut state = test_render_state(Some(view.position));
    state.snapshot.tick = u64::MAX;
    state.snapshot.simulated_seconds = u64::MAX as f64 / 60.0;
    state.snapshot.seed = u64::MAX;
    state.snapshot.speed = 256.0;
    state.selection = Some(WORLD_GENERATION_BOUNDS);
    state.selection_valid = false;
    state.generation_status = GenerationStatus::WorkerUnavailable;
    state.spawn_message = Some("SPAWN FAILED - CELL IS OCCUPIED BY AGENT 4294967295".to_owned());
    let mut hud_text = String::with_capacity(HUD_TEXT_CAPACITY);
    write_hud_text(&mut hud_text, &world, &state);
    let mut instances = Vec::with_capacity(SCREEN_OVERLAY_CAPACITY);
    build_screen_overlay(
        &mut instances,
        &hud_text,
        Some(&budget_text),
        &state,
        1_920,
        1_080,
    );
    assert!(
        instances.len() > 4_096,
        "the regression layout must exercise the former undersized budget"
    );
    assert!(instances.len() <= SCREEN_OVERLAY_CAPACITY);
}
