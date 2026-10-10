//! Engine side of life: who is how old, growing old, and dying of old age.

use crate::life::{ADULT_AGE, ELDER_AGE, Life, LifeView, SECONDS_PER_YEAR, Sex, old_age_risk};
use crate::wildlife::mix;
use crate::{AgentActivity, AgentId, DeathCause, Engine, SLEEP_HEALING};

/// Ticks in one year of life (one simulated hour).
pub(super) const TICKS_PER_YEAR: u64 = SECONDS_PER_YEAR as u64 * 60;

impl Engine {
    /// Sex, age, and life stage of `agent`, if it exists.
    pub fn life(&self, agent: AgentId) -> Option<LifeView> {
        self.population.view(agent)?;
        Some(self.life_of(agent).view(self.life_seconds()))
    }

    pub(super) fn life_of(&self, agent: AgentId) -> Life {
        self.lives
            .get(agent.get() as usize)
            .copied()
            .flatten()
            .unwrap_or_else(|| {
                Life::starting(self.config.seed, agent, !self.minds.is_founder(agent))
            })
    }

    /// The personality `agent` was born with: inherited from its parents if it
    /// was born during the run, otherwise drawn from the seed.
    pub(super) fn innate_personality(&self, agent: AgentId) -> crate::Personality {
        self.life_of(agent)
            .inherited
            .unwrap_or_else(|| crate::Personality::of(self.config.seed, agent))
    }

    /// Years of age of `agent` now.
    pub(super) fn age_of(&self, agent: AgentId) -> u32 {
        self.life_of(agent).age(self.life_seconds())
    }

    pub(super) fn life_seconds(&self) -> i64 {
        (self.time.ticks() / 60) as i64
    }

    /// Health a full night's sleep gives back: less and less past `ELDER_AGE`.
    pub(super) fn sleep_healing(&self, agent: AgentId) -> u16 {
        let frailty = self.age_of(agent).saturating_sub(ELDER_AGE).min(25) as u16;
        SLEEP_HEALING - SLEEP_HEALING * frailty * 2 / 100
    }

    /// How much better or worse than average `agent` strikes (chance out of
    /// 100): men are somewhat stronger on average, the young and the old weaker.
    pub(super) fn strength(&self, agent: AgentId) -> i64 {
        let life = self.life_of(agent);
        let age = life.age(self.life_seconds());
        let sex = match life.sex {
            Sex::Male => 5,
            Sex::Female => -5,
        };
        let age_penalty = if age < ADULT_AGE {
            15
        } else {
            i64::from(age.saturating_sub(ELDER_AGE).min(20))
        };
        sex - age_penalty
    }

    /// Once a year: the old may die of old age.
    pub(super) fn age_people(&mut self) {
        let now = self.time.ticks();
        if now % TICKS_PER_YEAR != 0 {
            return;
        }
        let year = now / TICKS_PER_YEAR;
        let living: Vec<AgentId> = self
            .population
            .views(usize::MAX)
            .filter(|view| view.activity != AgentActivity::Dead)
            .map(|view| view.id)
            .collect();
        for agent in living {
            let risk = old_age_risk(self.age_of(agent));
            let roll = mix(self.config.seed ^ 0x4f4c_4441 ^ year ^ u64::from(agent.get()) << 32);
            if roll % 1_000 >= risk {
                continue;
            }
            self.cancel_construction(agent);
            if let Some(record) =
                self.population
                    .finalize_death(self.time, agent, DeathCause::OldAge)
            {
                self.death_records.push(record);
            }
        }
    }
}
