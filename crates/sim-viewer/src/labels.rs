//! Plain-language names for what the viewer shows: people, materials, animals,
//! goals, reasons, and words. Presentation only.

use sim_core::{
    AgentActivity, AgentId, AnimalMode, BiomeType, Concept, DeathCause, FeatureKind, GestureTopic,
    LandmarkKind, LifeStage, LifeView, Material, Mime, Personality, PhysicalGoal, PolicyReason,
    Sex, SleepQuality, SpawnKind, Species, StructureKind, SurfaceType, VocalForm,
};

/// "Woman, 34", "Boy, 9", "Baby girl, 1".
pub fn who(life: LifeView) -> String {
    let noun = match (life.stage, life.sex) {
        (LifeStage::Baby, Sex::Female) => "Baby girl",
        (LifeStage::Baby, Sex::Male) => "Baby boy",
        (LifeStage::Child, Sex::Female) => "Girl",
        (LifeStage::Child, Sex::Male) => "Boy",
        (LifeStage::Adult | LifeStage::Elder, Sex::Female) => "Woman",
        (LifeStage::Adult | LifeStage::Elder, Sex::Male) => "Man",
    };
    format!("{noun}, {}", life.age)
}

pub fn person(agent: AgentId) -> String {
    format!("Person {}", agent.get())
}

/// A word as the agents say it, in quotes (`"nomu"`).
pub fn word(form: VocalForm) -> String {
    format!("\"{}\"", form.name())
}

pub const fn material(material: Material) -> &'static str {
    match material {
        Material::Berries => "berries",
        Material::Bitterberries => "bitter berries",
        Material::Wood => "wood",
        Material::Stone => "stone",
        Material::Meat => "meat",
    }
}

pub const fn species(species: Species) -> &'static str {
    match species {
        Species::Deer => "deer",
        Species::Wolf => "wolf",
    }
}

pub const fn species_plural(species: Species) -> &'static str {
    match species {
        Species::Deer => "deer",
        Species::Wolf => "wolves",
    }
}

pub const fn concept(concept: Concept) -> &'static str {
    match concept {
        Concept::Water => "water",
        Concept::Berries => "berries",
        Concept::Wood => "wood",
        Concept::Stone => "stone",
        Concept::Home => "home",
        Concept::Been => "been there",
        Concept::Bitterberries => "bitter berries",
        Concept::Deer => "deer",
        Concept::Wolf => "wolf",
        Concept::Fire => "fire",
    }
}

/// At most `GRID_LABEL_CHARS - 1` characters, for the word grid.
pub const fn concept_short(concept: Concept) -> &'static str {
    match concept {
        Concept::Been => "searched",
        Concept::Bitterberries => "bitter",
        other => self::concept(other),
    }
}

pub const fn place(kind: LandmarkKind) -> &'static str {
    match kind {
        LandmarkKind::Water => "water",
        LandmarkKind::Berries => "berries",
        LandmarkKind::Wood => "wood",
        LandmarkKind::Stone => "stone",
        LandmarkKind::Shelter => "a hut",
        LandmarkKind::Bitterberries => "bitter berries",
        LandmarkKind::Hearth => "a hearth",
    }
}

pub const fn topic(topic: GestureTopic) -> &'static str {
    match topic {
        GestureTopic::Place(kind) => place(kind),
        GestureTopic::Explored => "ground already searched",
        GestureTopic::Animal(animal) => species(animal),
    }
}

/// What someone visibly does while signalling.
pub const fn mime(mime: Mime) -> &'static str {
    match mime {
        Mime::Scoop => "drinking",
        Mime::PickAndChew => "eating",
        Mime::Chop => "chopping",
        Mime::Strike => "knocking stones",
        Mime::RestHead => "sleeping",
        Mime::Sweep => "sweeping",
        Mime::Retch => "retching",
        Mime::Snarl => "snarling",
        Mime::Spear => "throwing",
        Mime::Warm => "warming hands",
    }
}

pub const fn death(cause: DeathCause) -> &'static str {
    match cause {
        DeathCause::Dehydration => "died of thirst",
        DeathCause::Exposure => "died of cold",
        DeathCause::Starvation => "starved",
        DeathCause::Exhaustion => "died of exhaustion",
        DeathCause::Injury => "died of injuries",
        DeathCause::OldAge => "died of old age",
    }
}

/// What a person is setting out to do, as a short sentence.
pub const fn goal(goal: PhysicalGoal) -> &'static str {
    match goal {
        PhysicalGoal::SeekWater => "Going for water",
        PhysicalGoal::SeekFood => "Looking for food",
        PhysicalGoal::GatherMaterial => "Gathering materials",
        PhysicalGoal::Eat => "Eating",
        PhysicalGoal::Drink => "Drinking",
        PhysicalGoal::Sleep => "Going to sleep",
        PhysicalGoal::SeekShelter => "Heading for shelter",
        PhysicalGoal::BuildShelter => "Building a hut",
        PhysicalGoal::Wait => "Resting",
        PhysicalGoal::Incapacitated => "Collapsed",
        PhysicalGoal::Explore => "Exploring",
        PhysicalGoal::Signal => "Pointing something out",
        PhysicalGoal::Hunt => "Hunting",
        PhysicalGoal::BuildHearth => "Building a hearth",
        PhysicalGoal::WarmUp => "Warming up by a fire",
    }
}

/// Why, when it adds something to the goal.
pub const fn reason(reason: PolicyReason) -> Option<&'static str> {
    Some(match reason {
        PolicyReason::ThirstThreshold => "thirsty",
        PolicyReason::HungerThreshold => "hungry",
        PolicyReason::RestThreshold => "tired",
        PolicyReason::ExposureThreshold => "cold",
        PolicyReason::ShelterMaterials => "collecting for a hut",
        PolicyReason::HearthMaterials => "collecting for a hearth",
        PolicyReason::Retry => "trying again",
        PolicyReason::RememberedPlace => "going to a place it remembers",
        PolicyReason::ToldPlace => "going where someone pointed",
        PolicyReason::PrepareTrip => "stocking up for a trip",
        PolicyReason::Sharing => "showing others a place",
        PolicyReason::Returning => "heading back towards water",
        PolicyReason::Visiting => "visiting a friend",
        PolicyReason::Following => "following a parent",
        PolicyReason::Begging => "asking for food",
        PolicyReason::Fleeing => "running from an animal",
        PolicyReason::Warning => "warning others",
        PolicyReason::Recruiting => "calling others to hunt",
        PolicyReason::InitialDecision
        | PolicyReason::NoUrgentNeed
        | PolicyReason::RouteArrived
        | PolicyReason::ActionCompleted
        | PolicyReason::Hunting
        | PolicyReason::Warming => return None,
    })
}

pub const fn activity(activity: AgentActivity) -> &'static str {
    match activity {
        AgentActivity::Idle => "idle",
        AgentActivity::Moving => "walking",
        AgentActivity::Gathering => "gathering",
        AgentActivity::Building => "building",
        AgentActivity::Sleeping => "sleeping",
        AgentActivity::Incapacitated => "collapsed",
        AgentActivity::Dead => "dead",
    }
}

pub const fn sleep(quality: SleepQuality) -> &'static str {
    match quality {
        SleepQuality::OpenGround => "out in the open",
        SleepQuality::Sheltered => "under shelter",
    }
}

pub const fn animal_mode(mode: AnimalMode) -> Option<&'static str> {
    match mode {
        AnimalMode::Grazing => None,
        AnimalMode::Fleeing => Some("running away"),
        AnimalMode::Hunting => Some("hunting"),
        AnimalMode::Resting => Some("resting"),
    }
}

pub const fn structure(kind: StructureKind) -> &'static str {
    match kind {
        StructureKind::Shelter => "Hut",
        StructureKind::Hearth => "Hearth",
    }
}

pub const fn feature(kind: FeatureKind) -> &'static str {
    match kind {
        FeatureKind::Tree => "Tree (wood)",
        FeatureKind::Rock => "Rock (stone)",
        FeatureKind::BerryBush => "Berry bush",
        FeatureKind::BitterBush => "Bitter berry bush",
    }
}

pub const fn spawn_kind(kind: SpawnKind) -> &'static str {
    match kind {
        SpawnKind::Tree => "Tree",
        SpawnKind::BerryBush => "Berry bush",
        SpawnKind::Rock => "Rock",
        SpawnKind::Water => "Water",
    }
}

pub const fn terrain(surface: SurfaceType, biome: BiomeType) -> &'static str {
    match surface {
        SurfaceType::DeepWater => "Deep water",
        SurfaceType::ShallowWater => match biome {
            BiomeType::Ocean => "Sea shallows",
            BiomeType::River => "River",
            _ => "Lake",
        },
        SurfaceType::Hill => "Hills",
        SurfaceType::Rock => "Bare rock",
        SurfaceType::SnowIce => "Snow and ice",
        SurfaceType::Sand | SurfaceType::Soil => match biome {
            BiomeType::Beach => "Beach",
            BiomeType::Desert => "Desert",
            BiomeType::Grassland => "Grassland",
            BiomeType::Savanna => "Savanna",
            BiomeType::Forest => "Forest",
            BiomeType::Wetland => "Wetland",
            BiomeType::Tundra => "Tundra",
            BiomeType::Alpine => "Alpine",
            BiomeType::Ocean | BiomeType::Lake | BiomeType::River => "Shore",
        },
    }
}

/// A trait this far from the average (128) is pronounced enough to name.
const NOTABLE_TRAIT_DEVIATION: u8 = 48;

/// The trait that deviates most from average (ties go to the earlier trait in
/// curiosity, caution, sociability, diligence order), or `balanced`.
pub fn personality(personality: Personality) -> &'static str {
    let traits = [
        (personality.curiosity, "curious", "a homebody"),
        (personality.caution, "careful", "daring"),
        (personality.sociability, "sociable", "a loner"),
        (personality.diligence, "hard-working", "easygoing"),
    ];
    let mut summary = "balanced";
    let mut strongest = NOTABLE_TRAIT_DEVIATION - 1;
    for (value, high, low) in traits {
        let deviation = value.abs_diff(128);
        if deviation > strongest {
            strongest = deviation;
            summary = if value > 128 { high } else { low };
        }
    }
    summary
}

/// Simulated time as `1h 05m` (or `45s` in the first minute).
pub fn duration(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    match (total / 3_600, total / 60 % 60) {
        (0, 0) => format!("{}s", total % 60),
        (0, minutes) => format!("{minutes}m {:02}s", total % 60),
        (hours, minutes) => format!("{hours}h {minutes:02}m"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn personality_names_the_most_pronounced_trait_deterministically() {
        let average = Personality {
            curiosity: 128,
            caution: 128,
            sociability: 128,
            diligence: 128,
        };
        assert_eq!(personality(average), "balanced");
        assert_eq!(
            personality(Personality {
                curiosity: 200,
                ..average
            }),
            "curious"
        );
        assert_eq!(
            personality(Personality {
                caution: 20,
                sociability: 236,
                ..average
            }),
            "daring",
            "ties go to the earlier trait"
        );
        assert_eq!(
            personality(Personality {
                sociability: 40,
                ..average
            }),
            "a loner"
        );
    }

    #[test]
    fn durations_read_naturally() {
        assert_eq!(duration(12.7), "12s");
        assert_eq!(duration(65.0), "1m 05s");
        assert_eq!(duration(3_600.0 * 2.0 + 60.0 * 7.0 + 9.0), "2h 07m");
    }

    #[test]
    fn people_are_described_by_sex_and_age() {
        let life = |sex, age| LifeView {
            sex,
            age,
            stage: LifeStage::of(age),
        };
        assert_eq!(who(life(Sex::Female, 34)), "Woman, 34");
        assert_eq!(who(life(Sex::Male, 9)), "Boy, 9");
        assert_eq!(who(life(Sex::Female, 1)), "Baby girl, 1");
        assert_eq!(who(life(Sex::Male, 70)), "Man, 70");
    }

    #[test]
    fn reasons_that_repeat_the_goal_are_left_out() {
        assert_eq!(reason(PolicyReason::Hunting), None);
        assert_eq!(
            reason(PolicyReason::ToldPlace),
            Some("going where someone pointed")
        );
    }
}
