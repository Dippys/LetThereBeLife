//! Packed terrain cells, surface/biome classification, climate samples, and
//! generated water identity.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SurfaceType {
    DeepWater,
    ShallowWater,
    Sand,
    Soil,
    Hill,
    Rock,
    SnowIce,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BiomeType {
    Ocean,
    Lake,
    River,
    Beach,
    Desert,
    Grassland,
    Savanna,
    Forest,
    Wetland,
    Tundra,
    Alpine,
}

/// Packed rendered surface and environmental biome classification.
///
/// The low nibble stores [`SurfaceType`] and the high nibble stores
/// [`BiomeType`]. Construction is private to this crate, so every bit pattern
/// held by a public [`TerrainCell`] resolves to valid enums through safe
/// accessors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct TerrainClass(pub(crate) u8);

impl TerrainClass {
    pub(crate) const fn new(surface: SurfaceType, biome: BiomeType) -> Self {
        Self((biome as u8) << 4 | surface as u8)
    }

    pub fn surface(self) -> SurfaceType {
        match self.0 & 0x0f {
            0 => SurfaceType::DeepWater,
            1 => SurfaceType::ShallowWater,
            2 => SurfaceType::Sand,
            3 => SurfaceType::Soil,
            4 => SurfaceType::Hill,
            5 => SurfaceType::Rock,
            6 => SurfaceType::SnowIce,
            _ => unreachable!("TerrainClass is constructed from SurfaceType"),
        }
    }

    pub fn biome(self) -> BiomeType {
        match self.0 >> 4 {
            0 => BiomeType::Ocean,
            1 => BiomeType::Lake,
            2 => BiomeType::River,
            3 => BiomeType::Beach,
            4 => BiomeType::Desert,
            5 => BiomeType::Grassland,
            6 => BiomeType::Savanna,
            7 => BiomeType::Forest,
            8 => BiomeType::Wetland,
            9 => BiomeType::Tundra,
            10 => BiomeType::Alpine,
            _ => unreachable!("TerrainClass is constructed from BiomeType"),
        }
    }

    pub const fn packed(self) -> u8 {
        self.0
    }

    pub(crate) fn from_packed(packed: u8) -> Option<Self> {
        let class = Self(packed);
        (matches!(packed & 0x0f, 0..=6) && matches!(packed >> 4, 0..=10)).then_some(class)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PrevailingWind {
    Southeast,
    Northwest,
}

/// Diagnostic climate inputs for a generated cell. Temperature is derived
/// from the same 32-cell lattice used during classification; moisture remains
/// the compact value retained by `TerrainCell`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct ClimateSample {
    pub temperature: u16,
    pub moisture: u8,
    pub wind: PrevailingWind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct TerrainCell {
    pub elevation: u16,
    pub moisture: u8,
    pub(crate) class: TerrainClass,
}

impl TerrainCell {
    pub(crate) const fn new(
        elevation: u16,
        moisture: u8,
        surface: SurfaceType,
        biome: BiomeType,
    ) -> Self {
        Self {
            elevation,
            moisture,
            class: TerrainClass::new(surface, biome),
        }
    }

    pub fn surface(self) -> SurfaceType {
        self.class.surface()
    }

    pub fn biome(self) -> BiomeType {
        self.class.biome()
    }

    pub const fn classification(self) -> TerrainClass {
        self.class
    }
}

/// Generated water-body identity at one resident cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WaterSource {
    Ocean,
    Lake,
    River,
}

impl WaterSource {
    /// The first physical-agent loop can drink untreated lake and river water;
    /// ocean water is deliberately not drinkable.
    pub const fn is_drinkable(self) -> bool {
        matches!(self, Self::Lake | Self::River)
    }
}

pub(crate) fn water_source(cell: TerrainCell) -> Option<WaterSource> {
    match cell.biome() {
        BiomeType::Ocean => Some(WaterSource::Ocean),
        BiomeType::Lake => Some(WaterSource::Lake),
        BiomeType::River => Some(WaterSource::River),
        _ => None,
    }
}
