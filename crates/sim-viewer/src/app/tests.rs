//! Viewer app scheduling, cancellation, dirty-region, and selection tests.

use super::*;
use super::{
    selection::{bounded_selection, selection_is_valid},
    world_loading::{union_bounds, union_load_bounds},
};
use sim_core::{
    CHUNK_SIZE, ChunkPresence, EngineConfig, MAX_CHUNKS_PER_GENERATION, WORLD_GENERATION_BOUNDS,
    World, WorldConfig,
};

fn engine_with_world(width: u32, height: u32) -> Engine {
    Engine::new(EngineConfig {
        seed: 7,
        ticks_per_second: 60,
        world: WorldConfig::new(width, height).expect("small test world is valid"),
    })
}

#[test]
fn streamed_changes_coalesce_into_one_dirty_region() {
    let mut app = ViewerApp::new(engine_with_world(64, 64), None, None);
    app.dirty = false;
    app.mark_world_changed(WorldRect {
        min: WorldPosition { x: -32, y: 8 },
        max: WorldPosition { x: 4, y: 72 },
    });
    app.mark_world_changed(WorldRect {
        min: WorldPosition { x: 2, y: -4 },
        max: WorldPosition { x: 96, y: 12 },
    });

    assert!(app.dirty);
    assert_eq!(
        app.pending_world_changes,
        Some(WorldRect {
            min: WorldPosition { x: -32, y: -4 },
            max: WorldPosition { x: 96, y: 72 },
        })
    );
}

#[test]
fn bootstrap_pager_materializes_once_then_releases_its_queue() {
    let mut app = ViewerApp::new(engine_with_world(64, 64), None, None);
    let requests = app
        .take_next_bootstrap_requests()
        .expect("bootstrap request is valid")
        .expect("unloaded bootstrap area has work");
    assert_eq!(requests.len(), 4);
    assert!(requests.iter().all(|request| {
        app.engine
            .world()
            .initial_bounds()
            .contains_rect(request.bounds())
    }));

    let seed = app.engine.config().seed;
    let loads = requests
        .into_iter()
        .map(|request| World::generate_chunk_load(seed, request))
        .collect();
    assert_eq!(app.engine.apply_world_chunk_loads(loads), Ok(4));

    assert_eq!(
        app.take_next_bootstrap_requests()
            .expect("loaded bootstrap page is valid"),
        None
    );
    assert!(app.bootstrap_pager.is_none());
}

#[test]
fn fully_resident_startup_skips_bootstrap_paging() {
    let mut engine = engine_with_world(64, 64);
    engine.materialize_initial_area().unwrap();

    let app = ViewerApp::new(engine, None, None);

    assert!(app.bootstrap_pager.is_none());
    assert_eq!(app.population_status, PopulationStatus::Ready);
}

#[test]
fn manual_generation_preempts_bootstrap_paging() {
    let mut app = ViewerApp::new(engine_with_world(64, 64), None, None);
    let manual_bounds = WorldRect {
        min: WorldPosition { x: 64, y: 0 },
        max: WorldPosition { x: 128, y: 64 },
    };
    app.pending_manual = Some(
        app.engine
            .world()
            .missing_chunk_load_requests(manual_bounds)
            .expect("manual selection is valid"),
    );

    assert!(app.schedule_generation());
    assert!(matches!(
        app.active_generation.as_ref().map(|active| active.kind),
        Some(GenerationKind::Manual)
    ));
    assert!(app.pending_manual.is_none());

    app.cancel_active_generation();
    assert!(
        app.active_generation
            .as_ref()
            .is_some_and(|active| active.discard_loads)
    );
}

#[test]
fn no_generation_is_scheduled_without_bootstrap_or_manual_work() {
    let mut app = ViewerApp::new(engine_with_world(64, 64), None, None);
    app.bootstrap_pager = None;

    assert!(!app.schedule_generation());
    assert!(app.active_generation.is_none());
    assert!(!app.generation_is_pending());
}

#[test]
fn requeued_bootstrap_page_is_preserved_until_higher_priority_manual_work_runs() {
    let mut app = ViewerApp::new(engine_with_world(64, 64), None, None);
    let bootstrap = app
        .engine
        .world()
        .missing_chunk_load_requests(app.engine.world().initial_bounds())
        .expect("bootstrap page is valid");
    let manual_bounds = WorldRect {
        min: WorldPosition { x: 64, y: 0 },
        max: WorldPosition { x: 128, y: 64 },
    };
    let manual = app
        .engine
        .world()
        .missing_chunk_load_requests(manual_bounds)
        .expect("manual selection is valid");

    app.requeue_generation(GenerationKind::Bootstrap, bootstrap);
    app.pending_manual = Some(manual);

    assert!(app.schedule_generation());
    assert!(matches!(
        app.active_generation.as_ref().map(|active| active.kind),
        Some(GenerationKind::Manual)
    ));
    assert!(app.pending_bootstrap.is_some());
}

#[test]
fn background_cancellation_never_discards_manual_generation() {
    let mut app = ViewerApp::new(engine_with_world(64, 64), None, None);
    app.active_generation = Some(ActiveGeneration {
        id: 10,
        kind: GenerationKind::Manual,
        discard_loads: false,
    });
    app.cancel_background_generation();
    assert!(
        app.active_generation
            .as_ref()
            .is_some_and(|active| !active.discard_loads)
    );

    app.active_generation = Some(ActiveGeneration {
        id: 11,
        kind: GenerationKind::Bootstrap,
        discard_loads: false,
    });
    app.cancel_background_generation();
    assert!(
        app.active_generation
            .as_ref()
            .is_some_and(|active| active.discard_loads)
    );
}

#[test]
fn cancellation_clears_an_active_right_drag_before_it_can_queue_manual_work() {
    let mut app = ViewerApp::new(engine_with_world(64, 64), None, None);
    let selection = WorldRect {
        min: WorldPosition { x: 0, y: 0 },
        max: WorldPosition { x: 64, y: 64 },
    };
    app.active_generation = Some(ActiveGeneration {
        id: 12,
        kind: GenerationKind::Bootstrap,
        discard_loads: false,
    });
    app.pending_manual = Some(Vec::new());
    app.pending_bootstrap = Some(Vec::new());
    app.selection_start = Some(selection.min);
    app.selection = Some(selection);
    app.selection_validation = Some(SelectionValidation {
        bounds: selection,
        world_revision: 0,
        valid: true,
    });

    app.cancel_pending_generation();

    assert!(
        app.active_generation
            .as_ref()
            .is_some_and(|active| active.discard_loads)
    );
    assert!(app.pending_manual.is_none());
    assert!(app.pending_bootstrap.is_none());
    assert!(app.selection_start.is_none());
    assert!(app.selection.is_none());
    assert!(app.selection_validation.is_none());
    assert!(app.bootstrap_pager.is_none());
}

#[test]
fn streamed_load_bounds_accumulate_exact_clipped_bootstrap_coverage() {
    let world = World::new(7, WorldConfig::new(96, 64).expect("test world is valid"));
    let expected = world.initial_bounds();
    let loads = world
        .missing_chunk_load_requests(expected)
        .expect("bootstrap request is valid")
        .into_iter()
        .map(|request| World::generate_chunk_load(7, request))
        .collect::<Vec<_>>();

    assert_eq!(union_load_bounds(&loads), Some(expected));
    assert_eq!(
        union_bounds(
            WorldRect {
                min: WorldPosition { x: -32, y: 8 },
                max: WorldPosition { x: 4, y: 72 },
            },
            WorldRect {
                min: WorldPosition { x: 2, y: -4 },
                max: WorldPosition { x: 96, y: 12 },
            },
        ),
        WorldRect {
            min: WorldPosition { x: -32, y: -4 },
            max: WorldPosition { x: 96, y: 72 },
        }
    );
}

#[test]
fn selection_validation_and_inspection_distinguish_unloaded_bootstrap_tiles() {
    let world = World::new(7, WorldConfig::new(96, 64).expect("test world is valid"));
    let oversized = WorldRect {
        min: WorldPosition { x: 128, y: 0 },
        max: WorldPosition {
            x: 192,
            y: (MAX_CHUNKS_PER_GENERATION as i64 + 1) * CHUNK_SIZE,
        },
    };

    assert!(selection_is_valid(&world, Some(world.initial_bounds())));
    assert!(!selection_is_valid(&world, Some(oversized)));

    let inspection = world
        .inspect_chunk_at(WorldPosition { x: 47, y: 0 })
        .expect("configured position is inspectable");
    assert_eq!(inspection.presence, ChunkPresence::PartialInitialUnloaded);
    assert!(world.cell(WorldPosition { x: 47, y: 0 }).is_none());
}

#[test]
fn right_drag_selection_caps_at_the_world_boundary() {
    let world = World::new(7, WorldConfig::new(64, 64).expect("test world is valid"));
    let start = WorldPosition {
        x: WORLD_GENERATION_BOUNDS.max.x - 2,
        y: WORLD_GENERATION_BOUNDS.min.y + 2,
    };
    let expected = WorldRect {
        min: WorldPosition {
            x: WORLD_GENERATION_BOUNDS.max.x - 2,
            y: WORLD_GENERATION_BOUNDS.min.y,
        },
        max: WorldPosition {
            x: WORLD_GENERATION_BOUNDS.max.x,
            y: WORLD_GENERATION_BOUNDS.min.y + 3,
        },
    };

    assert_eq!(
        bounded_selection(
            start,
            WorldPosition {
                x: WORLD_GENERATION_BOUNDS.max.x + 1_000,
                y: WORLD_GENERATION_BOUNDS.min.y - 1_000,
            },
        ),
        Some(expected)
    );
    assert!(world.missing_chunk_load_requests(expected).is_ok());
}

#[test]
fn right_drag_selection_does_not_start_outside_the_world_boundary() {
    assert_eq!(
        bounded_selection(
            WorldPosition {
                x: WORLD_GENERATION_BOUNDS.max.x,
                y: 0,
            },
            WorldPosition { x: 0, y: 0 },
        ),
        None
    );
}
