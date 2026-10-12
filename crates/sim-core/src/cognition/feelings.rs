//! Feelings (docs/plans/FEELINGS.md): how a person feels, raised by what
//! happens to it and fading with time, faster for the steady. Each feeling
//! changes choices it already makes, and the strongest one shows on its face,
//! where others can read it, or misread it.

/// The feelings a person can have about its own life (feelings toward
/// particular people live with each acquaintance).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[repr(u8)]
pub enum Feeling {
    Fear,
    Contentment,
    Misery,
    Boredom,
    Loneliness,
    Grief,
    Disgust,
    Worry,
    Hope,
    Frustration,
    Surprise,
    Relief,
    Pride,
    Shame,
    Envy,
    Jealousy,
}

impl Feeling {
    pub const COUNT: usize = 16;
    pub const ALL: [Self; Self::COUNT] = [
        Self::Fear,
        Self::Contentment,
        Self::Misery,
        Self::Boredom,
        Self::Loneliness,
        Self::Grief,
        Self::Disgust,
        Self::Worry,
        Self::Hope,
        Self::Frustration,
        Self::Surprise,
        Self::Relief,
        Self::Pride,
        Self::Shame,
        Self::Envy,
        Self::Jealousy,
    ];

    /// Seconds for the feeling to halve on an average temperament.
    const fn half_life(self) -> u32 {
        match self {
            Self::Surprise => 120,
            Self::Fear | Self::Disgust | Self::Relief => 300,
            Self::Misery | Self::Frustration => 900,
            Self::Hope | Self::Shame | Self::Envy => 1_200,
            Self::Jealousy => 2_400,
            Self::Contentment | Self::Boredom | Self::Pride => 1_800,
            Self::Loneliness | Self::Worry => 3_600,
            Self::Grief => 7_200,
        }
    }

    /// The feeling someone reading this one on a face can mistake it for:
    /// fear and frustration both look tense, grief and misery both look low,
    /// contentment and pride both look easy.
    pub const fn lookalike(self) -> Self {
        match self {
            Self::Fear => Self::Frustration,
            Self::Frustration => Self::Fear,
            Self::Grief => Self::Misery,
            Self::Misery => Self::Grief,
            Self::Contentment => Self::Pride,
            Self::Pride => Self::Contentment,
            Self::Worry => Self::Fear,
            Self::Disgust => Self::Misery,
            Self::Shame => Self::Misery,
            Self::Boredom => Self::Contentment,
            Self::Loneliness => Self::Misery,
            Self::Hope => Self::Contentment,
            Self::Surprise => Self::Fear,
            Self::Relief => Self::Contentment,
            Self::Envy | Self::Jealousy => Self::Frustration,
        }
    }
}

/// A feeling this strong shows on the face.
pub const SHOWS: u8 = 96;
/// A feeling this strong takes over what a person does.
pub const STRONG: u8 = 128;

/// 20 bytes (16 levels and a time): a level per feeling (0-255) and when they were last faded.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Feelings {
    levels: [u8; Feeling::COUNT],
    since: u32,
}

impl Feelings {
    pub const fn level(&self, feeling: Feeling) -> u8 {
        self.levels[feeling as usize]
    }

    pub const fn strong(&self, feeling: Feeling) -> bool {
        self.level(feeling) >= STRONG
    }

    /// Lets feelings fade up to simulated second `now`; the `steadiness`
    /// trait (0-255) sets how fast (a quarter faster to twice as slow).
    pub(crate) fn fade(&mut self, now: u32, steadiness: u8) {
        let elapsed = now.saturating_sub(self.since);
        if elapsed == 0 {
            return;
        }
        self.since = now;
        // Steady people get over things faster.
        let pace = 64 + u32::from(steadiness) / 2;
        for feeling in Feeling::ALL {
            let half = (feeling.half_life() * 128 / pace).max(1);
            let level = &mut self.levels[feeling as usize];
            let halvings = elapsed / half;
            let mut value = if halvings >= 8 {
                0
            } else {
                u32::from(*level) >> halvings
            };
            // The rest of a half-life takes off its share in a straight line.
            value -= value * (elapsed % half) / (2 * half);
            *level = value as u8;
        }
    }

    /// Something raised `feeling` by `amount`.
    pub(crate) fn feel(&mut self, feeling: Feeling, amount: u8) {
        let level = &mut self.levels[feeling as usize];
        *level = level.saturating_add(amount);
    }

    /// Something eased `feeling` by `amount`.
    pub(crate) fn ease(&mut self, feeling: Feeling, amount: u8) {
        let level = &mut self.levels[feeling as usize];
        *level = level.saturating_sub(amount);
    }

    /// The strongest feeling, if it shows on the face.
    pub fn shown(&self) -> Option<Feeling> {
        Feeling::ALL
            .into_iter()
            .map(|feeling| (self.level(feeling), feeling))
            .filter(|&(level, _)| level >= SHOWS)
            .max_by_key(|&(level, feeling)| (level, std::cmp::Reverse(feeling)))
            .map(|(_, feeling)| feeling)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feelings_rise_with_what_happens_and_fade_faster_for_the_steady() {
        let mut steady = Feelings::default();
        let mut moody = Feelings::default();
        steady.feel(Feeling::Fear, 200);
        moody.feel(Feeling::Fear, 200);
        assert_eq!(steady.shown(), Some(Feeling::Fear));
        steady.fade(600, 255);
        moody.fade(600, 0);
        assert!(steady.level(Feeling::Fear) < moody.level(Feeling::Fear));
        moody.fade(10_000, 0);
        assert_eq!(moody.level(Feeling::Fear), 0, "it passes");
        assert_eq!(moody.shown(), None);
        moody.feel(Feeling::Grief, 250);
        moody.ease(Feeling::Grief, 50);
        assert_eq!(moody.level(Feeling::Grief), 200);
        assert_eq!(Feeling::Fear.lookalike(), Feeling::Frustration);
        assert_eq!(std::mem::size_of::<Feelings>(), 20);
    }
}
