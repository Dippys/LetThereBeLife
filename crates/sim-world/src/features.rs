//! Sparse generated surface features, the materials they yield, and what those
//! materials physically do. Agents don't know these properties; they learn them.

use crate::WorldPosition;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FeatureKind {
    Tree,
    Rock,
    BerryBush,
    /// Looks much like a berry bush; its berries make you sick.
    BitterBush,
}

/// A gatherable, carryable material. Behavior should depend on its
/// [`MaterialProperties`], never on which material it is.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[repr(u8)]
pub enum Material {
    Berries,
    Bitterberries,
    Wood,
    Stone,
    /// From a carcass. It spoils where it lies, but keeps once carried.
    Meat,
    /// A sharp flake knapped from stone.
    Blade,
    /// A deer's or wolf's skin, cut off with a blade: worn, it keeps the cold off.
    Hide,
    /// A wooden shaft shaped to a point with a blade: strikes land more often.
    Spear,
    /// Woven from twigs: holds more than two hands can.
    Basket,
    /// Meat cooked over a fire: more filling, and it keeps.
    Roast,
    /// Meat smoked slowly by a fire: keeps for a season or more.
    DriedMeat,
    /// Berries dried by a fire: keep for a season.
    DriedBerries,
    /// Dried meat pounded with dried berries: very filling, keeps for a year.
    Pemmican,
    /// Strips of hide twisted together: binds things.
    Cord,
    /// A blade bound to a handle: chops far faster than a hand-held flake.
    Axe,
    /// A blade bound to a shaft: strikes surer and deeper than a sharpened stick.
    StoneSpear,
    /// A stick weighted with a stone: digs quickly, for sowing and burying.
    DiggingStick,
    /// A hide tied on with cord: keeps the cold off better than a loose skin.
    Cloak,
}

/// How a material is taken from where it's found: the motion anyone watching
/// would recognize.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handling {
    /// Picked by hand.
    Pick,
    /// Hacked off with blows.
    Chop,
    /// Knocked loose.
    Strike,
    /// Cut from a carcass.
    Carve,
}

/// What a material physically does when eaten or used. Need units match the
/// 0–10,000 need scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaterialProperties {
    /// How it's gathered.
    pub handling: Handling,
    /// Found at fixed places (bushes, trees, rocks) worth remembering, rather
    /// than on something that soon goes away (a carcass).
    pub fixed_source: bool,
    /// Grows back only outside winter (fruit).
    pub seasonal: bool,
    /// Seconds one unit keeps a fire burning (0 = doesn't burn).
    pub fuel_seconds: u32,
    /// Has a cutting edge: carrying one makes strikes land more often.
    pub cutting: bool,
    /// Hunger relieved by eating one unit.
    pub nutrition: u16,
    /// Sickness from eating one unit: added to thirst and to tiredness.
    pub toxicity: u16,
    /// Usable as building material (wind-blocking structure).
    pub builds: bool,
    /// Seconds for its source to grow back one unit (0 = never).
    pub regrow_seconds: u32,
    /// Cold kept off the wearer while carried (less chill on exposure).
    pub warmth: i8,
    /// Added to the chance (in %) a strike at an animal lands, while carried.
    pub strike: u8,
    /// Extra things its carrier can hold.
    pub room: u8,
    /// Days it keeps before going bad, carried (0 = it keeps): each dawn about
    /// one part in this many of what's carried rots.
    pub keeps_days: u8,
    /// Times as much wood or stone its carrier hacks off at a go (0 = no help).
    pub chop: u8,
    /// Digs: sowing and burying go twice as fast while it's carried.
    pub digs: bool,
    /// About one use in this many breaks it (0 = it doesn't wear out).
    pub wear_odds: u8,
}

impl MaterialProperties {
    /// Kept on the body rather than set down or put away: a cutting edge, or
    /// something worn for warmth.
    pub const fn worn(self) -> bool {
        self.cutting || self.warmth > 0 || self.strike > 0 || self.room > 0
    }
}

impl Material {
    pub const COUNT: usize = 18;
    pub const ALL: [Self; Self::COUNT] = [
        Self::Berries,
        Self::Bitterberries,
        Self::Wood,
        Self::Stone,
        Self::Meat,
        Self::Blade,
        Self::Hide,
        Self::Spear,
        Self::Basket,
        Self::Roast,
        Self::DriedMeat,
        Self::DriedBerries,
        Self::Pemmican,
        Self::Cord,
        Self::Axe,
        Self::StoneSpear,
        Self::DiggingStick,
        Self::Cloak,
    ];

    pub const fn properties(self) -> MaterialProperties {
        match self {
            Self::Berries => MaterialProperties {
                cutting: false,
                fuel_seconds: 0,
                seasonal: true,
                handling: Handling::Pick,
                fixed_source: true,
                nutrition: 4_000,
                toxicity: 0,
                builds: false,
                regrow_seconds: 1_200,
                warmth: 0,
                strike: 0,
                room: 0,
                keeps_days: 8,
                chop: 0,
                digs: false,
                wear_odds: 0,
            },
            Self::Bitterberries => MaterialProperties {
                cutting: false,
                fuel_seconds: 0,
                seasonal: true,
                handling: Handling::Pick,
                fixed_source: true,
                nutrition: 1_200,
                toxicity: 2_500,
                builds: false,
                regrow_seconds: 1_200,
                warmth: 0,
                strike: 0,
                room: 0,
                keeps_days: 8,
                chop: 0,
                digs: false,
                wear_odds: 0,
            },
            Self::Wood => MaterialProperties {
                cutting: false,
                fuel_seconds: 1_800,
                seasonal: false,
                handling: Handling::Chop,
                fixed_source: true,
                nutrition: 0,
                toxicity: 0,
                builds: true,
                regrow_seconds: 2 * 3_600,
                warmth: 0,
                strike: 0,
                room: 0,
                keeps_days: 0,
                chop: 0,
                digs: false,
                wear_odds: 0,
            },
            Self::Stone => MaterialProperties {
                cutting: false,
                fuel_seconds: 0,
                seasonal: false,
                handling: Handling::Strike,
                fixed_source: true,
                nutrition: 0,
                toxicity: 0,
                builds: false,
                regrow_seconds: 0,
                warmth: 0,
                strike: 0,
                room: 0,
                keeps_days: 0,
                chop: 0,
                digs: false,
                wear_odds: 0,
            },
            Self::Meat => MaterialProperties {
                cutting: false,
                fuel_seconds: 0,
                seasonal: false,
                handling: Handling::Carve,
                fixed_source: false,
                nutrition: 6_000,
                toxicity: 0,
                builds: false,
                regrow_seconds: 0,
                warmth: 0,
                strike: 0,
                room: 0,
                keeps_days: 3,
                chop: 0,
                digs: false,
                wear_odds: 0,
            },
            Self::Blade => MaterialProperties {
                cutting: true,
                fuel_seconds: 0,
                seasonal: false,
                handling: Handling::Strike,
                fixed_source: false,
                nutrition: 0,
                toxicity: 0,
                builds: false,
                regrow_seconds: 0,
                warmth: 0,
                strike: 0,
                room: 0,
                keeps_days: 0,
                chop: 2,
                digs: false,
                wear_odds: 6,
            },
            Self::Hide => MaterialProperties {
                cutting: false,
                fuel_seconds: 0,
                seasonal: false,
                handling: Handling::Carve,
                fixed_source: false,
                nutrition: 0,
                toxicity: 0,
                builds: false,
                regrow_seconds: 0,
                warmth: 1,
                strike: 0,
                room: 0,
                keeps_days: 0,
                chop: 0,
                digs: false,
                wear_odds: 0,
            },
            Self::Spear => MaterialProperties {
                cutting: false,
                fuel_seconds: 0,
                seasonal: false,
                handling: Handling::Pick,
                fixed_source: false,
                nutrition: 0,
                toxicity: 0,
                builds: false,
                regrow_seconds: 0,
                warmth: 0,
                strike: 15,
                room: 0,
                keeps_days: 0,
                chop: 0,
                digs: false,
                wear_odds: 15,
            },
            Self::Basket => MaterialProperties {
                cutting: false,
                fuel_seconds: 0,
                seasonal: false,
                handling: Handling::Pick,
                fixed_source: false,
                nutrition: 0,
                toxicity: 0,
                builds: false,
                regrow_seconds: 0,
                warmth: 0,
                strike: 0,
                room: 6,
                keeps_days: 0,
                chop: 0,
                digs: false,
                wear_odds: 0,
            },
            Self::Roast => MaterialProperties {
                cutting: false,
                fuel_seconds: 0,
                seasonal: false,
                handling: Handling::Carve,
                fixed_source: false,
                nutrition: 7_000,
                toxicity: 0,
                builds: false,
                regrow_seconds: 0,
                warmth: 0,
                strike: 0,
                room: 0,
                keeps_days: 15,
                chop: 0,
                digs: false,
                wear_odds: 0,
            },
            Self::DriedMeat => MaterialProperties {
                cutting: false,
                fuel_seconds: 0,
                seasonal: false,
                handling: Handling::Pick,
                fixed_source: false,
                nutrition: 5_000,
                toxicity: 0,
                builds: false,
                regrow_seconds: 0,
                warmth: 0,
                strike: 0,
                room: 0,
                keeps_days: 60,
                chop: 0,
                digs: false,
                wear_odds: 0,
            },
            Self::DriedBerries => MaterialProperties {
                cutting: false,
                fuel_seconds: 0,
                seasonal: false,
                handling: Handling::Pick,
                fixed_source: false,
                nutrition: 3_500,
                toxicity: 0,
                builds: false,
                regrow_seconds: 0,
                warmth: 0,
                strike: 0,
                room: 0,
                keeps_days: 40,
                chop: 0,
                digs: false,
                wear_odds: 0,
            },
            Self::Pemmican => MaterialProperties {
                cutting: false,
                fuel_seconds: 0,
                seasonal: false,
                handling: Handling::Pick,
                fixed_source: false,
                nutrition: 9_000,
                toxicity: 0,
                builds: false,
                regrow_seconds: 0,
                warmth: 0,
                strike: 0,
                room: 0,
                keeps_days: 200,
                chop: 0,
                digs: false,
                wear_odds: 0,
            },
            Self::Cord => MaterialProperties {
                cutting: false,
                fuel_seconds: 0,
                seasonal: false,
                handling: Handling::Pick,
                fixed_source: false,
                nutrition: 0,
                toxicity: 0,
                builds: false,
                regrow_seconds: 0,
                warmth: 0,
                strike: 0,
                room: 0,
                keeps_days: 0,
                chop: 0,
                digs: false,
                wear_odds: 0,
            },
            Self::Axe => MaterialProperties {
                cutting: true,
                fuel_seconds: 0,
                seasonal: false,
                handling: Handling::Pick,
                fixed_source: false,
                nutrition: 0,
                toxicity: 0,
                builds: false,
                regrow_seconds: 0,
                warmth: 0,
                strike: 0,
                room: 0,
                keeps_days: 0,
                chop: 3,
                digs: false,
                wear_odds: 20,
            },
            Self::StoneSpear => MaterialProperties {
                cutting: false,
                fuel_seconds: 0,
                seasonal: false,
                handling: Handling::Pick,
                fixed_source: false,
                nutrition: 0,
                toxicity: 0,
                builds: false,
                regrow_seconds: 0,
                warmth: 0,
                strike: 30,
                room: 0,
                keeps_days: 0,
                chop: 0,
                digs: false,
                wear_odds: 30,
            },
            Self::DiggingStick => MaterialProperties {
                cutting: false,
                fuel_seconds: 0,
                seasonal: false,
                handling: Handling::Pick,
                fixed_source: false,
                nutrition: 0,
                toxicity: 0,
                builds: false,
                regrow_seconds: 0,
                warmth: 0,
                strike: 0,
                room: 0,
                keeps_days: 0,
                chop: 0,
                digs: true,
                wear_odds: 20,
            },
            Self::Cloak => MaterialProperties {
                cutting: false,
                fuel_seconds: 0,
                seasonal: false,
                handling: Handling::Pick,
                fixed_source: false,
                nutrition: 0,
                toxicity: 0,
                builds: false,
                regrow_seconds: 0,
                warmth: 3,
                strike: 0,
                room: 0,
                keeps_days: 0,
                chop: 0,
                digs: false,
                wear_odds: 0,
            },
        }
    }
}

/// The physical situation making something takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Method {
    /// Nothing but hands.
    Hand,
    /// Shaped with a cutting edge carried.
    Edge,
    /// Over a burning fire beside you, quickly.
    Fire,
    /// By a burning fire beside you, slowly, in its smoke.
    Smoke,
}

impl Method {
    pub const COUNT: usize = 4;
    pub const ALL: [Self; Self::COUNT] = [Self::Hand, Self::Edge, Self::Fire, Self::Smoke];

    /// How many times longer than plain work it takes.
    pub const fn slowness(self) -> u64 {
        match self {
            Self::Hand | Self::Edge | Self::Fire => 1,
            Self::Smoke => 4,
        }
    }
}

/// One way of making something: what goes in (inputs of count 0 are unused),
/// how, and how many come out. Physics, not knowledge: people discover these
/// by trying and by watching each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recipe {
    pub inputs: [(Material, u8); 3],
    pub method: Method,
    pub output: Material,
    pub makes: u8,
}

/// The kinds of thing that go into something, as a set: ascending, unused
/// places `None` at the end.
pub type Kinds = [Option<Material>; 3];

/// `kinds` in canonical order (ascending, `None` last, no repeats).
pub fn canonical(mut kinds: Kinds) -> Kinds {
    kinds.sort_by_key(|kind| kind.map_or(u8::MAX, |material| material as u8));
    if kinds[1] == kinds[0] {
        kinds[1] = None;
    }
    if kinds[2] == kinds[1] || kinds[2] == kinds[0] {
        kinds[2] = None;
    }
    kinds.sort_by_key(|kind| kind.map_or(u8::MAX, |material| material as u8));
    kinds
}

const fn recipe(input: (Material, u8), method: Method, output: Material) -> Recipe {
    Recipe {
        inputs: [input, (input.0, 0), (input.0, 0)],
        method,
        output,
        makes: 1,
    }
}

const fn assembled(parts: [Material; 3], method: Method, output: Material) -> Recipe {
    Recipe {
        inputs: [(parts[0], 1), (parts[1], 1), (parts[2], 1)],
        method,
        output,
        makes: 1,
    }
}

/// Everything that can be made.
pub const RECIPES: [Recipe; 12] = [
    recipe((Material::Stone, 1), Method::Hand, Material::Blade),
    recipe((Material::Wood, 2), Method::Hand, Material::Basket),
    recipe((Material::Wood, 2), Method::Edge, Material::Spear),
    recipe((Material::Meat, 1), Method::Fire, Material::Roast),
    recipe((Material::Meat, 1), Method::Smoke, Material::DriedMeat),
    recipe(
        (Material::Berries, 2),
        Method::Smoke,
        Material::DriedBerries,
    ),
    Recipe {
        inputs: [
            (Material::DriedMeat, 1),
            (Material::DriedBerries, 1),
            (Material::DriedMeat, 0),
        ],
        method: Method::Hand,
        output: Material::Pemmican,
        makes: 2,
    },
    Recipe {
        inputs: [
            (Material::Hide, 1),
            (Material::Hide, 0),
            (Material::Hide, 0),
        ],
        method: Method::Edge,
        output: Material::Cord,
        makes: 3,
    },
    assembled(
        [Material::Blade, Material::Wood, Material::Cord],
        Method::Hand,
        Material::Axe,
    ),
    assembled(
        [Material::Blade, Material::Wood, Material::Cord],
        Method::Edge,
        Material::StoneSpear,
    ),
    Recipe {
        inputs: [
            (Material::Wood, 1),
            (Material::Stone, 1),
            (Material::Wood, 0),
        ],
        method: Method::Hand,
        output: Material::DiggingStick,
        makes: 1,
    },
    Recipe {
        inputs: [
            (Material::Hide, 1),
            (Material::Cord, 1),
            (Material::Hide, 0),
        ],
        method: Method::Hand,
        output: Material::Cloak,
        makes: 1,
    },
];

impl Recipe {
    /// The kinds of thing that go in, as a set (order doesn't matter).
    pub fn kinds(&self) -> Kinds {
        canonical(
            self.inputs
                .map(|(material, count)| (count > 0).then_some(material)),
        )
    }

    /// The recipe for these kinds of input by this method, if physics has one.
    pub fn find(kinds: Kinds, method: Method) -> Option<(usize, Recipe)> {
        let kinds = canonical(kinds);
        RECIPES
            .iter()
            .copied()
            .enumerate()
            .find(|(_, recipe)| recipe.method == method && recipe.kinds() == kinds)
    }
}

impl Material {
    /// The first recipe that makes this, if people can make it.
    pub const fn recipe(self) -> Option<Recipe> {
        let mut index = 0;
        while index < RECIPES.len() {
            if RECIPES[index].output as u8 == self as u8 {
                return Some(RECIPES[index]);
            }
            index += 1;
        }
        None
    }

    /// What it's mainly made from, if people make it.
    pub const fn made_from(self) -> Option<(Material, u8)> {
        match self.recipe() {
            Some(recipe) => Some(recipe.inputs[0]),
            None => None,
        }
    }
}

/// Generated maximum yield before any future sparse depletion state is applied.
///
/// Capacities are abstract gathering units. They belong to the versioned base
/// generator; remaining quantity, removal, and regrowth must live in a sparse
/// mutable layer keyed by feature position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct BaseResource {
    pub capacity: u16,
    pub kind: Material,
}

impl FeatureKind {
    pub const fn base_resource(self) -> BaseResource {
        match self {
            Self::Tree => BaseResource {
                capacity: 120,
                kind: Material::Wood,
            },
            Self::Rock => BaseResource {
                capacity: 80,
                kind: Material::Stone,
            },
            Self::BerryBush => BaseResource {
                capacity: 12,
                kind: Material::Berries,
            },
            Self::BitterBush => BaseResource {
                capacity: 12,
                kind: Material::Bitterberries,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Feature {
    pub position: WorldPosition,
    pub kind: FeatureKind,
}

impl Feature {
    /// Stable generated identity within one seed and generator revision.
    ///
    /// Future persisted sparse deltas must additionally record the generator
    /// version; the world position itself is the current feature key.
    pub const fn identity(self) -> WorldPosition {
        self.position
    }

    pub const fn base_resource(self) -> BaseResource {
        self.kind.base_resource()
    }
}
