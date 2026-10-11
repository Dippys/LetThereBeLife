//! Interface tests: the person panel's sentences, tooltips, clickable regions,
//! and the technical readout.

use sim_core::{
    AcquaintanceView, AffordanceView, AgentActivity, AgentId, AgentView, AnimalMode, Concept,
    DeathCause, DeathRecord, ExplorationHeading, FaunaView, HealthStatus, HealthView,
    InventoryView, LEXICON_SLOTS, LexiconEntryView, Material, MentalMapView, NeedLevelView,
    Personality, PhysicalGoal, PhysicalNeedsView, PhysicalPolicyView, PolicyReason, SimTime,
    Species, StructureKind, StructureState, VocalForm, World, WorldConfig, WorldPosition,
};

use super::test_render_state;
use crate::feed::{FeedEntry, Tone};
use crate::render::colors;
use crate::render::details::write_details;
use crate::render::ui::{
    Hit, Row, build_interface, hover_lines, info_rows, person_rows, status_lines,
};
use crate::render::{
    AgentInspection, BuildTool, GenerationStatus, Hover, MemoryInspection, UiAction,
};

fn word(form: u8, concept: Concept, positive: u16, contradictory: u16) -> LexiconEntryView {
    LexiconEntryView {
        form: VocalForm(form),
        concept,
        positive,
        contradictory,
        heard: positive + contradictory,
        successes: 0,
        failures: 0,
    }
}

fn friend(id: u32, familiarity: u8, trust: u8) -> AcquaintanceView {
    AcquaintanceView {
        agent: AgentId::new(id),
        familiarity,
        trust,
        last_seen_position: None,
        last_seen_second: 0,
        tie: None,
        owed: 0,
        name: None,
    }
}

fn level(value: u16, threshold: u16) -> NeedLevelView {
    NeedLevelView {
        value,
        rate_per_period: 4,
        threshold,
        threshold_reached: value >= threshold,
    }
}

fn mind(lexicon: Vec<LexiconEntryView>, acquaintances: Vec<AcquaintanceView>) -> MentalMapView {
    MentalMapView {
        agent: AgentId::new(7),
        personality: Personality {
            curiosity: 210,
            ..Personality::AVERAGE
        },
        landmarks: Vec::new(),
        explored_tiles: 0,
        child: true,
        affordances: vec![
            AffordanceView {
                material: Material::Berries,
                feeds: 3_800,
                sickens: 0,
                evidence: 4,
            },
            AffordanceView {
                material: Material::Bitterberries,
                feeds: 800,
                sickens: 2_400,
                evidence: 8,
            },
        ],
        fauna: vec![
            FaunaView {
                species: Species::Deer,
                prey: 200,
                danger: 0,
                evidence: 4,
            },
            FaunaView {
                species: Species::Wolf,
                prey: 0,
                danger: 220,
                evidence: 8,
            },
        ],
        knows_hearths: true,
        knows_knapping: true,
        knows_huts: true,
        acquaintances,
        lexicon,
    }
}

fn person() -> AgentInspection {
    let view = AgentView {
        id: AgentId::new(7),
        position: WorldPosition { x: 12, y: -9 },
        activity: AgentActivity::Moving,
    };
    AgentInspection {
        view,
        needs: Some(PhysicalNeedsView {
            agent: view.id,
            at: SimTime::from_ticks(90),
            hunger: level(7_500, 6_000),
            thirst: level(9_500, 6_000),
            rest: level(1_000, 8_000),
            exposure: level(0, 7_000),
            next_threshold: None,
        }),
        inventory: Some(InventoryView::of(&[
            (Material::Berries, 2),
            (Material::Wood, 3),
        ])),
        health: Some(HealthView {
            agent: view.id,
            value: 9_000,
            status: HealthStatus::Healthy,
            next_consequence: None,
        }),
        policy: Some(PhysicalPolicyView {
            agent: view.id,
            goal: PhysicalGoal::SeekFood,
            reason: PolicyReason::ToldPlace,
            target: Some(WorldPosition { x: 20, y: -4 }),
            committed: true,
            retry_count: 0,
            exploration_heading: ExplorationHeading::NorthEast,
        }),
        sleep: None,
        death: None,
        life: Some(sim_core::LifeView {
            name: sim_core::Name(5),
            sex: sim_core::Sex::Female,
            age: 9,
            stage: sim_core::LifeStage::Child,
        }),
        motherhood: None,
        memory: Some(MemoryInspection::from_view(&mind(
            vec![
                word(1, Concept::Water, 9, 1),
                // Same net evidence but less positive evidence: loses.
                word(2, Concept::Water, 8, 0),
                // Contested links are not words.
                word(3, Concept::STONE, 4, 4),
                word(0, Concept::WOLF, 9, 0),
            ],
            vec![
                friend(3, sim_core::FRIEND_FAMILIARITY, 100),
                friend(9, u8::MAX, 150),
                friend(4, 1, 200),
                friend(5, 30, 20),
                AcquaintanceView {
                    tie: Some(sim_core::Tie::Parent),
                    name: Some(sim_core::Name(1)),
                    ..friend(2, u8::MAX, 220)
                },
            ],
        ))),
    }
}

#[test]
fn the_person_panel_says_what_they_do_feel_and_believe_in_plain_words() {
    let rows = person_rows(&person());
    let text = |row: &Row| match row {
        Row::Title(text) | Row::Text(text, _) => vec![text.clone()],
        Row::Pair(label, value) => vec![format!("{label}: {value}")],
        Row::Status { doing, why } => std::iter::once(doing.clone()).chain(why.clone()).collect(),
        _ => Vec::new(),
    };
    let lines: Vec<String> = rows.iter().flat_map(text).collect();
    for expected in [
        "Kata",
        "Girl, 9 · curious · Person 7",
        "Looking for food",
        "Going where someone pointed",
        "Carrying: 2 berries, 3 wood (5 of 12)",
        "Eats: berries",
        "Avoids: bitter berries",
        "Hunts: deer",
        "Fears: wolves",
        "Knows how to make fire",
        "Knows how to build huts",
        "Knows how to knap stone blades",
        "Family: Mata (parent)",
        "Friends: Person 9, Person 3",
        "Distrusts: Person 5",
        "Knows: 5 people",
    ] {
        assert!(
            lines.iter().any(|line| line == expected),
            "{expected}: {lines:?}"
        );
    }
    let words = rows
        .iter()
        .find_map(|row| match row {
            Row::Grid(cells) => Some(cells.clone()),
            _ => None,
        })
        .expect("a word grid");
    assert_eq!(
        words,
        [
            ("water", format!("\"{}\"", VocalForm(1).name())),
            ("wolf", format!("\"{}\"", VocalForm(0).name())),
        ]
    );

    let bar = |label: &str| {
        rows.iter()
            .find_map(|row| match row {
                Row::Bar {
                    label: name,
                    fill,
                    mark,
                    color,
                } if *name == label => Some((*fill, *mark, *color)),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(bar("Health"), (0.9, None, colors::UI_GOOD));
    assert_eq!(
        bar("Hunger"),
        (0.75, Some(0.6), colors::UI_WARN),
        "past its threshold"
    );
    assert_eq!(
        bar("Thirst").2,
        colors::UI_BAD,
        "halfway from threshold to the limit"
    );
    assert_eq!(bar("Tiredness").2, colors::UI_NEED);
}

#[test]
fn the_dead_get_a_cause_and_nothing_else_and_the_unknowing_say_so() {
    let mut dead = person();
    dead.view.activity = AgentActivity::Dead;
    dead.death = Some(DeathRecord {
        agent: dead.view.id,
        cause: DeathCause::Dehydration,
        at: SimTime::from_ticks(5),
        position: dead.view.position,
    });
    let rows = person_rows(&dead);
    assert_eq!(
        rows.last(),
        Some(&Row::Status {
            doing: "Died of thirst".to_owned(),
            why: None
        })
    );
    assert!(!rows.iter().any(|row| matches!(row, Row::Bar { .. })));

    let mut blank = person();
    let mut view = mind(Vec::new(), Vec::new());
    view.affordances.clear();
    view.fauna.clear();
    view.knows_hearths = false;
    view.knows_knapping = false;
    view.knows_huts = false;
    blank.memory = Some(MemoryInspection::from_view(&view));
    let rows = person_rows(&blank);
    assert!(rows.contains(&Row::Text("Nothing yet".to_owned(), colors::UI_DIM)));
    assert!(rows.contains(&Row::Text("No words yet".to_owned(), colors::UI_DIM)));
    assert!(rows.contains(&Row::Pair("Friends", "none yet".to_owned())));
}

#[test]
fn the_lexicon_copy_is_bounded_and_words_need_net_evidence() {
    let mut crowded = vec![word(0, Concept::STONE, 1, 0); LEXICON_SLOTS + 4];
    crowded[LEXICON_SLOTS] = word(0, Concept::BERRIES, 9, 0);
    let memory = MemoryInspection::from_view(&mind(crowded, Vec::new()));
    assert_eq!(memory.lexicon().len(), LEXICON_SLOTS);
    assert_eq!(memory.word_for(Concept::BERRIES), None);
    assert_eq!(memory.word_for(Concept::STONE), Some(VocalForm(0)));
}

#[test]
fn tooltips_name_what_is_under_the_mouse() {
    assert_eq!(
        hover_lines(&Hover::Animal {
            species: Species::Wolf,
            mode: AnimalMode::Hunting
        }),
        ("Wolf".to_owned(), Some("hunting".to_owned()))
    );
    assert_eq!(
        hover_lines(&Hover::Resource {
            label: "Berry bush",
            remaining: Some((3, Material::Berries))
        }),
        ("Berry bush".to_owned(), Some("3 berries left".to_owned()))
    );
    assert_eq!(
        hover_lines(&Hover::Resource {
            label: "Tree (wood)",
            remaining: Some((0, Material::Wood))
        })
        .1,
        Some("picked clean".to_owned())
    );
    assert_eq!(
        hover_lines(&Hover::Structure {
            kind: StructureKind::Hearth,
            state: StructureState::UnderConstruction,
            burning: Some(600),
            stored: sim_core::InventoryView::default()
        }),
        ("Hearth".to_owned(), Some("being built".to_owned()))
    );
    assert_eq!(
        hover_lines(&Hover::Carcass { meat: 4 }).1,
        Some("4 meat left".to_owned())
    );
    assert_eq!(
        hover_lines(&Hover::Person {
            id: AgentId::new(2),
            name: None,
            activity: AgentActivity::Sleeping
        })
        .0,
        "Person 2"
    );
}

fn interface(state: &crate::render::RenderState) -> Vec<Hit> {
    let (width, height) = (1_280_u32, 800_u32);
    let view = state.camera.view(width, height, 64, 64);
    let mut instances = Vec::new();
    let mut hits = Vec::new();
    build_interface(&mut instances, &mut hits, state, view, &[], None);
    assert!(!instances.is_empty());
    hits
}

fn action_at(hits: &[Hit], x: f32, y: f32) -> Option<Option<UiAction>> {
    hits.iter()
        .rev()
        .find(|hit| hit.contains(x, y))
        .map(|hit| hit.action)
}

fn center_of(hits: &[Hit], action: UiAction) -> (f32, f32) {
    let hit = hits
        .iter()
        .find(|hit| hit.action == Some(action))
        .unwrap_or_else(|| panic!("no {action:?} button"));
    (hit.x + hit.width / 2.0, hit.y + hit.height / 2.0)
}

#[test]
fn the_top_bar_has_working_buttons_and_blocks_map_clicks() {
    let state = test_render_state(None);
    let hits = interface(&state);
    for action in [
        UiAction::TogglePause,
        UiAction::Slower,
        UiAction::Faster,
        UiAction::Help,
    ] {
        let (x, y) = center_of(&hits, action);
        assert_eq!(action_at(&hits, x, y), Some(Some(action)));
    }
    let (_, bar_y) = center_of(&hits, UiAction::Help);
    assert_eq!(
        action_at(&hits, 640.0, bar_y),
        Some(None),
        "the bar itself blocks"
    );
    assert_eq!(action_at(&hits, 640.0, 400.0), None, "the map is free");
}

#[test]
fn panels_offer_their_actions_and_help_covers_everything() {
    let mut state = test_render_state(None);
    state.selected = Some(person());
    state.build = Some(BuildTool::Person);
    state.feed = vec![FeedEntry {
        text: "A wolf bit Person 7".to_owned(),
        tone: Tone::Bad,
        agent: Some(AgentId::new(7)),
        position: WorldPosition { x: 1, y: 2 },
        count: 1,
    }];
    let hits = interface(&state);
    for action in [
        UiAction::CloseSelected,
        UiAction::Follow,
        UiAction::NextPerson,
        UiAction::FeedEntry(0),
    ]
    .into_iter()
    .chain(BuildTool::ALL.map(UiAction::Tool))
    {
        let (x, y) = center_of(&hits, action);
        assert_eq!(action_at(&hits, x, y), Some(Some(action)));
    }

    state.help_open = true;
    let hits = interface(&state);
    assert_eq!(
        action_at(&hits, 2.0, 790.0),
        Some(Some(UiAction::Help)),
        "click outside closes"
    );
    assert_eq!(
        action_at(&hits, 640.0, 400.0),
        Some(None),
        "the help panel itself blocks"
    );
}

#[test]
fn details_report_simulation_and_cursor_cell_values() {
    let world = World::generate(7, WorldConfig::new(64, 64).unwrap());
    let mut state = test_render_state(Some(WorldPosition { x: 0, y: 0 }));
    state.selected = Some(person());
    let mut text = String::new();
    write_details(&mut text, &world, &state);
    for expected in [
        "TICK 3721  CLOCK 1m 02s  SEED 7  CHUNKS ",
        "CURSOR X 0  Y 0",
        "CHUNK X 0 Y 0  LOCAL 0,0",
        "SURFACE ",
        "TEMP ",
        "AGENT 7 AT 12,-9",
        "GOAL SeekFood  WHY ToldPlace",
        "COMMITTED  RETRIES 0  HEADING NE  TARGET 20,-4",
        "HUNGER 7500/6000 +4",
    ] {
        assert!(text.contains(expected), "{expected}: {text}");
    }

    let unloaded = World::new(7, WorldConfig::new(96, 64).unwrap());
    state.cursor_world = Some(WorldPosition { x: 47, y: 0 });
    state.generation_status = GenerationStatus::WorkerUnavailable;
    write_details(&mut text, &unloaded, &state);
    assert!(text.contains("PARTIAL INITIAL UNLOADED"));
    assert!(text.contains("CELL UNLOADED"));
}

#[test]
fn the_status_block_keeps_its_height_whatever_it_says() {
    assert_eq!(status_lines("Sleeping", None, 30).len(), 3);
    let long = status_lines(
        "Going to where a friend was last seen",
        Some("Heading for a place someone pointed out, a long way off past the river"),
        20,
    );
    assert_eq!(long.len(), 3);
    assert!(long[2].0.ends_with("..."), "{long:?}");
    assert!(long.iter().all(|(line, _)| line.chars().count() <= 20));
}

#[test]
fn the_info_box_tells_the_weather_and_how_people_are_doing() {
    let mut state = super::test_render_state(None);
    state.season = sim_core::Season::Winter;
    state.census.hearths = 3;
    state.census.fires_burning = 1;
    state.census.cold = 4;
    state.census.born_this_year = 2;
    let rows = info_rows(&state);
    let text: Vec<String> = rows
        .iter()
        .map(|(label, value)| format!("{label}: {value}"))
        .collect();
    for expected in [
        "Weather: Winter, freezing",
        "Bushes: bare, nothing grows back",
        "Fires: 1 burning of 3",
        "Feeling: 4 cold, 0 hungry, 0 thirsty, 0 tired",
        "This year: 2 born, 0 died",
    ] {
        assert!(
            text.iter().any(|line| line == expected),
            "{expected}: {text:?}"
        );
    }
}
