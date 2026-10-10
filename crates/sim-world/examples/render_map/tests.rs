//! Render-map option parsing, sampling, and report tests.

use super::*;

#[test]
fn view_validation_enforces_world_bounds_and_pixel_budget() {
    assert!(validate_view(WORLD_GENERATION_BOUNDS, 128).is_ok());
    assert!(
        validate_view(
            WorldRect {
                min: WorldPosition {
                    x: WORLD_GENERATION_BOUNDS.min.x - 1,
                    y: 0,
                },
                max: WorldPosition { x: 64, y: 64 },
            },
            1,
        )
        .unwrap_err()
        .contains("must remain inside")
    );
    assert!(
        validate_view(
            WorldRect {
                min: WorldPosition { x: 0, y: 0 },
                max: WorldPosition { x: 4_097, y: 4_097 },
            },
            1,
        )
        .unwrap_err()
        .contains("maximum")
    );
}

#[test]
fn ad_hoc_view_rejects_coordinate_overflow() {
    let error = parse_single_options(
        [
            "--min-x".to_owned(),
            i64::MAX.to_string(),
            "--width".to_owned(),
            "1".to_owned(),
        ]
        .into_iter(),
    )
    .unwrap_err();
    assert_eq!(error, "--min-x plus --width exceeds i64 coordinates");
}

#[test]
fn sampling_alignment_is_relative_to_the_view_origin_across_signed_chunks() {
    let origin = -1_003;
    let step = 7;
    for value in [-1_100, -1_024, -1_003, -1_000, -960, -1] {
        let aligned = align_up(value, origin, step).unwrap();
        assert!(aligned >= value);
        assert_eq!((aligned - origin).rem_euclid(step), 0);
        assert!(aligned - value < step);
    }
    assert_eq!(align_up(-1_024, origin, step), Ok(-1_024));
}

#[test]
fn canonical_views_cover_both_drainage_seam_axes() {
    let seam_x = FOCUSED_VIEWS
        .iter()
        .find(|view| view.name == "drainage-seam-x")
        .unwrap();
    let seam_y = FOCUSED_VIEWS
        .iter()
        .find(|view| view.name == "drainage-seam-y")
        .unwrap();
    assert!(seam_x.bounds.min.x < 0 && seam_x.bounds.max.x > 0);
    assert!(seam_y.bounds.min.y < 0 && seam_y.bounds.max.y > 0);
    assert!(
        FOCUSED_VIEWS
            .into_iter()
            .all(|view| view.validate().is_ok())
    );
}

#[test]
fn feature_ecology_views_are_full_resolution_and_visible() {
    let views: Vec<_> = FOCUSED_VIEWS
        .into_iter()
        .filter(|view| {
            matches!(
                view.name,
                "forest-features" | "outcrop-features" | "close-up"
            )
        })
        .collect();
    assert_eq!(views.len(), 3);
    assert!(
        views
            .iter()
            .all(|view| view.step == 1 && view.show_features)
    );
}

#[test]
fn representative_seed_contract_is_multi_seed_and_keeps_repository_seed() {
    assert_eq!(REPRESENTATIVE_SEEDS[0].seed, 1);
    assert!(REPRESENTATIVE_SEEDS.len() >= 4);
    let unique: std::collections::BTreeSet<_> = REPRESENTATIVE_SEEDS
        .into_iter()
        .map(|case| case.seed)
        .collect();
    assert_eq!(unique.len(), REPRESENTATIVE_SEEDS.len());
}

#[test]
fn output_metadata_is_byte_stable_for_equal_inputs() {
    let output = RenderedView {
        view: ReviewView::new("fixture", 7, (-64, -32, 128, 64), 4, true),
        relative_path: PathBuf::from("seed-7/fixture.bmp"),
        stats: SampleStats {
            surfaces: [1, 2, 3, 4, 5, 6, 7],
            biomes: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            features: [8, 9, 10, 0],
            samples: 28,
            sample_hash: 0x1234_5678_9abc_def0,
        },
        elapsed: Duration::from_secs(99),
    };
    let left = review_manifest("abc123+dirty", std::slice::from_ref(&output));
    let right = review_manifest("abc123+dirty", &[output]);
    assert_eq!(left, right);
    assert_eq!(
        left,
        "review_format\tsource_revision\tview\tseed\tmin_x\tmin_y\tmax_x\tmax_y\tstep\tcolumns\trows\tfeatures\tsample_hash\tpath\n3\tabc123+dirty\tfixture\t7\t-64\t-32\t64\t32\t4\t32\t16\ttrue\t123456789abcdef0\tseed-7/fixture.bmp\n"
    );
}

#[test]
fn semantic_sample_hash_is_deterministic_and_coordinate_sensitive() {
    let cell = ChunkGenerator::new(1, ChunkCoord { x: 0, y: 0 })
        .unwrap()
        .sample(ChunkLocalPosition { x: 0, y: 0 })
        .unwrap();
    let mut left = SampleStats::default();
    let mut right = SampleStats::default();
    left.record(-1, 2, cell);
    right.record(-1, 2, cell);
    assert_eq!(left, right);
    right.record(0, 2, cell);
    assert_ne!(left.sample_hash, right.sample_hash);
}
