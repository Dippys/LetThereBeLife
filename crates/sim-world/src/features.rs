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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Material {
    Berries,
    Bitterberries,
    Wood,
    Stone,
    /// From a carcass. It spoils where it lies, but keeps once carried.
    Meat,
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
    /// Hunger relieved by eating one unit.
    pub nutrition: u16,
    /// Sickness from eating one unit: added to thirst and to tiredness.
    pub toxicity: u16,
    /// Usable as building material (wind-blocking structure).
    pub builds: bool,
    /// Seconds for its source to grow back one unit (0 = never).
    pub regrow_seconds: u32,
}

impl Material {
    pub const COUNT: usize = 5;
    pub const ALL: [Self; Self::COUNT] = [
        Self::Berries,
        Self::Bitterberries,
        Self::Wood,
        Self::Stone,
        Self::Meat,
    ];

    pub const fn properties(self) -> MaterialProperties {
        match self {
            Self::Berries => MaterialProperties {
                seasonal: true,
                handling: Handling::Pick,
                fixed_source: true,
                nutrition: 4_000,
                toxicity: 0,
                builds: false,
                regrow_seconds: 600,
            },
            Self::Bitterberries => MaterialProperties {
                seasonal: true,
                handling: Handling::Pick,
                fixed_source: true,
                nutrition: 1_200,
                toxicity: 2_500,
                builds: false,
                regrow_seconds: 600,
            },
            Self::Wood => MaterialProperties {
                seasonal: false,
                handling: Handling::Chop,
                fixed_source: true,
                nutrition: 0,
                toxicity: 0,
                builds: true,
                regrow_seconds: 3_600,
            },
            Self::Stone => MaterialProperties {
                seasonal: false,
                handling: Handling::Strike,
                fixed_source: true,
                nutrition: 0,
                toxicity: 0,
                builds: false,
                regrow_seconds: 0,
            },
            Self::Meat => MaterialProperties {
                seasonal: false,
                handling: Handling::Carve,
                fixed_source: false,
                nutrition: 6_000,
                toxicity: 0,
                builds: false,
                regrow_seconds: 0,
            },
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
