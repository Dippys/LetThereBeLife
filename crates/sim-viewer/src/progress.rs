//! What the console shows while `--advance` fast-forwards: a live progress
//! line, a summary at the start of each year, and a line for every birth and
//! death. Plain ASCII, so it reads the same in any Windows console.

use std::io::Write;
use std::time::{Duration, Instant};

use sim_core::{Engine, FamilyEvent, Sex, StructureKind, StructureState};

use crate::labels;

/// How often the progress line is redrawn.
const REDRAW: Duration = Duration::from_millis(250);
const BAR_WIDTH: usize = 30;

pub struct Progress {
    total: u64,
    started: Instant,
    last_drawn: Option<Instant>,
    /// Width of the progress line on screen, to blank it fully.
    drawn_width: usize,
    deaths_seen: usize,
    year: i64,
    born_this_year: u32,
    died_this_year: u32,
}

impl Progress {
    pub fn new(engine: &Engine, total: u64) -> Self {
        let years = total as f64 / sim_core::TICKS_PER_YEAR as f64;
        println!();
        println!("  Let There Be Life");
        println!("  -----------------");
        println!(
            "  Fast-forwarding {years:.1} years (seed {}). The window opens when it's done;",
            engine.snapshot().seed
        );
        println!("  closing this console stops it.");
        println!();
        Self {
            total,
            started: Instant::now(),
            last_drawn: None,
            drawn_width: 0,
            deaths_seen: engine.death_records().len(),
            year: year_of(engine),
            born_this_year: 0,
            died_this_year: 0,
        }
    }

    /// Notes what the latest tick brought; `done` ticks of the run have passed.
    pub fn record(&mut self, engine: &Engine, done: u64) {
        // A new year closes the old one before anything that happened in it.
        let year = year_of(engine);
        if year != self.year {
            self.summarize_year(engine);
            self.year = year;
            self.born_this_year = 0;
            self.died_this_year = 0;
        }
        for event in engine.family_events() {
            if let FamilyEvent::Born { mother, sex, .. } = *event {
                self.born_this_year += 1;
                let child = match sex {
                    Sex::Female => "girl",
                    Sex::Male => "boy",
                };
                self.log(
                    engine,
                    &format!("{} gave birth to a {child}", name(engine, mother)),
                );
            }
        }
        let deaths = engine.death_records();
        for death in &deaths[self.deaths_seen.min(deaths.len())..] {
            self.died_this_year += 1;
            let age = engine
                .life(death.agent)
                .map(|life| format!(" at {}", life.age))
                .unwrap_or_default();
            self.log(
                engine,
                &format!(
                    "{}{age} {}",
                    name(engine, death.agent),
                    labels::death(death.cause)
                ),
            );
        }
        self.deaths_seen = deaths.len();
        if self
            .last_drawn
            .is_none_or(|drawn| drawn.elapsed() >= REDRAW)
        {
            self.draw(engine, done);
        }
    }

    /// Ends the progress line once the run is over.
    pub fn finish(&mut self, engine: &Engine) {
        self.draw(engine, self.total);
        println!();
        println!();
        println!(
            "  Done in {}. {} people alive. Opening the window...",
            clock(self.started.elapsed()),
            engine.snapshot().living_agent_count
        );
    }

    fn summarize_year(&mut self, engine: &Engine) {
        let snapshot = engine.snapshot();
        let now = (snapshot.tick / 60) as u32;
        let (mut huts, mut fires, mut burning) = (0, 0, 0);
        for structure in engine.structure_views(usize::MAX) {
            if structure.state != StructureState::Complete {
                continue;
            }
            match structure.kind {
                StructureKind::Shelter => huts += 1,
                StructureKind::Hearth => {
                    fires += 1;
                    burning += u32::from(structure.working(now));
                }
            }
        }
        let oldest = engine
            .agent_views(usize::MAX)
            .filter(|view| view.activity != sim_core::AgentActivity::Dead)
            .filter_map(|view| engine.life(view.id).map(|life| life.age))
            .max()
            .unwrap_or(0);
        self.line(&format!(
            "== Year {} ends: {} people ({} born, {} died), {huts} huts, {burning} of {fires} fires burning, oldest {oldest}",
            self.year + 1,
            snapshot.living_agent_count,
            self.born_this_year,
            self.died_this_year,
        ));
    }

    fn log(&mut self, engine: &Engine, text: &str) {
        let season = labels::season(engine.season());
        self.line(&format!(
            "   Year {}, {season}: {text}",
            year_of(engine) + 1
        ));
    }

    /// Prints a full line above the progress bar.
    fn line(&mut self, text: &str) {
        print!("\r{:width$}\r", "", width = self.drawn_width);
        println!("  {text}");
        self.last_drawn = None;
    }

    fn draw(&mut self, engine: &Engine, done: u64) {
        let fraction = (done as f64 / self.total.max(1) as f64).clamp(0.0, 1.0);
        let filled = (fraction * BAR_WIDTH as f64).round() as usize;
        let elapsed = self.started.elapsed();
        let left = if fraction > 0.001 {
            clock(elapsed.mul_f64((1.0 - fraction) / fraction))
        } else {
            "--".to_owned()
        };
        let text = format!(
            "  [{}{}] {:>3.0}%  Year {} {:<6}  {:>3} people  {} elapsed, about {} left",
            "#".repeat(filled),
            "-".repeat(BAR_WIDTH - filled),
            fraction * 100.0,
            year_of(engine) + 1,
            labels::season(engine.season()),
            engine.snapshot().living_agent_count,
            clock(elapsed),
            left,
        );
        // Pad over whatever the last, possibly longer, line left behind.
        let width = text.chars().count().max(self.drawn_width);
        print!("\r{text:width$}");
        self.drawn_width = width;
        let _ = std::io::stdout().flush();
        self.last_drawn = Some(Instant::now());
    }
}

fn year_of(engine: &Engine) -> i64 {
    engine.snapshot().simulated_seconds as i64 / sim_core::SECONDS_PER_YEAR
}

fn name(engine: &Engine, agent: sim_core::AgentId) -> String {
    engine
        .life(agent)
        .map_or_else(|| labels::person(agent), |life| life.name.spoken())
}

/// "4m 07s" or "1h 12m".
fn clock(duration: Duration) -> String {
    let seconds = duration.as_secs();
    match seconds / 3_600 {
        0 => format!("{}m {:02}s", seconds / 60, seconds % 60),
        hours => format!("{hours}h {:02}m", seconds / 60 % 60),
    }
}
