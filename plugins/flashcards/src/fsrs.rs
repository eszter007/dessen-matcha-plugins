//! FSRS-5, as matcha's lib/Flashcards/Fsrs.cpp: stability (days until recall falls to 90%) and
//! difficulty (1..10) per card; the next review is due when recall drops to the desired retention.
use libm::{expf, powf, roundf};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Rating {
    Again = 1,
    Hard = 2,
    Good = 3,
    Easy = 4,
}
impl Rating {
    pub fn from(i: u8) -> Rating {
        match i {
            1 => Rating::Again,
            2 => Rating::Hard,
            4 => Rating::Easy,
            _ => Rating::Good,
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct Memory {
    pub stability: f32,
    pub difficulty: f32,
}

#[derive(Clone, Copy)]
pub struct Params {
    pub retention: f32,
    pub max_interval: u16,
}
impl Default for Params {
    fn default() -> Self { Params { retention: 0.9, max_interval: 36500 } }
}

// FSRS-5 default weights (fsrs-rs 1.x / Anki 23.10+).
const W: [f32; 19] = [0.40255, 1.18385, 3.173, 15.69105, 7.1949, 0.5345, 1.4604, 0.0046, 1.54575, 0.1192, 1.01925,
                      1.9395, 0.11, 0.29605, 2.2698, 0.2315, 2.9898, 0.51655, 0.6621];
const DECAY: f32 = -0.5;
const FACTOR: f32 = 19.0 / 81.0; // so that R(S, S) = 0.9

fn clamp_d(d: f32) -> f32 { d.clamp(1.0, 10.0) }
fn initial_difficulty(r: Rating) -> f32 { W[4] - expf(W[5] * (r as i32 as f32 - 1.0)) + 1.0 }
fn next_difficulty(d: f32, r: Rating) -> f32 {
    let delta = -W[6] * (r as i32 as f32 - 3.0);
    let damped = d + delta * (10.0 - d) / 9.0;
    clamp_d(W[7] * initial_difficulty(Rating::Easy) + (1.0 - W[7]) * damped)
}
fn recall_stability(m: Memory, r: f32, rating: Rating) -> f32 {
    let hard = if rating == Rating::Hard { W[15] } else { 1.0 };
    let easy = if rating == Rating::Easy { W[16] } else { 1.0 };
    m.stability
        * (expf(W[8]) * (11.0 - m.difficulty) * powf(m.stability, -W[9]) * (expf(W[10] * (1.0 - r)) - 1.0) * hard * easy
            + 1.0)
}
fn forget_stability(m: Memory, r: f32) -> f32 {
    let s = W[11] * powf(m.difficulty, -W[12]) * (powf(m.stability + 1.0, W[13]) - 1.0) * expf(W[14] * (1.0 - r));
    s.min(m.stability)
}

pub fn retrievability(elapsed: f32, stability: f32) -> f32 {
    if stability <= 0.0 {
        return 0.0;
    }
    powf(1.0 + FACTOR * elapsed.max(0.0) / stability, DECAY)
}

pub fn interval_days(stability: f32, p: &Params) -> u16 {
    let retention = p.retention.clamp(0.5, 0.99);
    let days = stability / FACTOR * (powf(retention, 1.0 / DECAY) - 1.0);
    (roundf(days) as i64).clamp(1, (p.max_interval as i64).max(1)) as u16
}

pub fn initial(r: Rating) -> Memory { Memory { stability: W[r as usize - 1], difficulty: clamp_d(initial_difficulty(r)) } }

pub fn next(m: Memory, elapsed: f32, r: Rating) -> Memory {
    if m.stability <= 0.0 {
        return initial(r);
    }
    let difficulty = next_difficulty(m.difficulty, r);
    let stability = if elapsed < 1.0 {
        m.stability * expf(W[17] * (r as i32 as f32 - 3.0 + W[18]))
    } else {
        let rr = retrievability(elapsed, m.stability);
        if r == Rating::Again { forget_stability(m, rr) } else { recall_stability(m, rr, r) }
    };
    Memory { stability: stability.max(0.01), difficulty }
}

/// The interval each rating would give, Again..Easy; Hard <= Good < Easy.
pub fn preview_intervals(m: Memory, is_new: bool, elapsed: f32, p: &Params) -> [u16; 4] {
    let mut days = [0u16; 3];
    for (i, d) in days.iter_mut().enumerate() {
        let r = Rating::from(i as u8 + 2);
        let mm = if is_new { initial(r) } else { next(m, elapsed, r) };
        *d = interval_days(mm.stability, p);
    }
    days[0] = days[0].min(days[1]);
    if days[1] <= days[0] {
        days[1] = (days[0] as u32 + 1).min(p.max_interval as u32) as u16;
    }
    if days[2] <= days[1] {
        days[2] = (days[1] as u32 + 1).min(p.max_interval as u32) as u16;
    }
    [0, days[0], days[1], days[2]]
}
