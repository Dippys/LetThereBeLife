//! Engine side of life: who is how old, growing old, and dying of old age.

pub(super) use crate::life::TICKS_PER_YEAR;
use crate::life::{ADULT_AGE, ELDER_AGE, Life, LifeView, Sex, old_age_risk};
use crate::wildlife::mix;
use crate::{AgentActivity, AgentId, DeathCause, Engine, SLEEP_HEALING};

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
                Life::starting(self.config.life_seed, agent, !self.minds.is_founder(agent))
            })
    }

    /// The personality `agent` was born with: inherited from its parents if it
    /// was born during the run, otherwise drawn from the seed.
    pub(super) fn innate_personality(&self, agent: AgentId) -> crate::Personality {
        self.life_of(agent)
            .inherited
            .unwrap_or_else(|| crate::Personality::of(self.config.life_seed, agent))
    }

    /// Gives `agent` this personality, as if born with it.
    #[cfg(test)]
    pub(crate) fn set_personality_for_test(
        &mut self,
        agent: AgentId,
        personality: crate::Personality,
    ) {
        let mut life = self.life_of(agent);
        life.inherited = Some(personality);
        let index = agent.get() as usize;
        if self.lives.len() <= index {
            self.lives.resize(index + 1, None);
        }
        self.lives[index] = Some(life);
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

    /// Grown, and not pregnant: able to build and chop wood.
    pub(super) fn fit_for_heavy_work(&self, agent: AgentId) -> bool {
        self.age_of(agent) >= ADULT_AGE
            && !self
                .motherhood(agent)
                .is_some_and(|motherhood| motherhood.pregnant)
    }

    /// The season now.
    pub fn season(&self) -> crate::Season {
        crate::Season::at(self.time)
    }

    /// The weather now: the season's chill (plus night's), and night's drowsiness.
    pub(super) fn weather(&self) -> (i8, i8) {
        let night = self.nights && crate::season::is_night(self.time);
        let chill = self.season().chill() + if night { crate::season::NIGHT_CHILL } else { 0 };
        let drowsy = if night {
            crate::season::NIGHT_DROWSINESS
        } else {
            0
        };
        (chill, drowsy)
    }

    /// The weather as `agent` feels it: what it wears keeps some of the cold off.
    pub(super) fn weather_for(&self, agent: AgentId) -> (i8, i8) {
        let (chill, drowsy) = self.weather();
        let warmth = self.population.inventory(agent).map_or(0, |inventory| {
            inventory
                .carried()
                .map(|(material, _)| material.properties().warmth)
                .max()
                .unwrap_or(0)
        });
        let chill = if chill > 0 {
            chill.saturating_sub(warmth).max(0)
        } else {
            chill
        };
        (chill, drowsy)
    }

    /// Puts `agent`'s needs on the weather as it feels it now.
    pub(super) fn dress_for_weather(&mut self, agent: AgentId) {
        let weather = self.weather_for(agent);
        let _ = self
            .population
            .apply_chill(&mut self.scheduler, self.time, agent, weather);
    }

    /// Whether it's night now (never, with nights switched off).
    pub fn is_night(&self) -> bool {
        self.nights && crate::season::is_night(self.time)
    }

    /// At each change of season, nightfall and dawn, everyone's needs follow
    /// the new weather.
    pub(super) fn turn_season(&mut self) {
        let ticks = self.time.ticks();
        let new_season = ticks % crate::season::SEASON_TICKS == 0;
        let day = (ticks / 60) % crate::season::DAY_SECONDS;
        // Each dawn, food that goes bad rots a little.
        if ticks > 0 && ticks % 60 == 0 && day == 0 {
            self.food_rotted += u64::from(self.population.rot_carried());
            self.food_rotted += u64::from(self.structures.rot_stocks());
        }
        let dawn_or_dusk =
            self.nights && ticks % 60 == 0 && (day == 0 || day == crate::season::NIGHTFALL_SECONDS);
        if !new_season && !dawn_or_dusk {
            return;
        }
        let living: Vec<AgentId> = self
            .population
            .views(usize::MAX)
            .filter(|view| view.activity != AgentActivity::Dead)
            .map(|view| view.id)
            .collect();
        for agent in living {
            self.dress_for_weather(agent);
        }
        if !new_season {
            return;
        }
        // What was sown this year dies with the first frost.
        if self.season() == crate::Season::Winter {
            self.crops_withered += self.spawned_objects.wither_sown();
        }
        // Huts nobody has lived in for long fall down.
        for structure in self.structures.weather() {
            self.structure_diagnostics.push(crate::StructureDiagnostic {
                structure,
                at: self.time,
                kind: crate::StructureDiagnosticKind::Collapsed,
                refunded_wood: 0,
                refunded_stone: 0,
            });
        }
    }

    /// Once a year: the old may die of old age.
    pub(super) fn age_people(&mut self) {
        let now = self.time.ticks();
        if now % TICKS_PER_YEAR != 0 {
            return;
        }
        let year = now / TICKS_PER_YEAR;
        self.welcome_newcomers(year);
        self.fade_skills();
        let living: Vec<AgentId> = self
            .population
            .views(usize::MAX)
            .filter(|view| view.activity != AgentActivity::Dead)
            .map(|view| view.id)
            .collect();
        for agent in living {
            let risk = old_age_risk(self.age_of(agent));
            let roll =
                mix(self.config.life_seed ^ 0x4f4c_4441 ^ year ^ u64::from(agent.get()) << 32);
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
