use crate::geometry::{Point, upper_pool};
use crate::penalty::{self, Penalty, Winner};

#[derive(Clone, Copy)]
pub struct Located {
    pub winner: Winner,
    pub rectangle: usize,
    pub penalty: Penalty,
    pub cap: usize,
}

pub struct Extremes {
    pub cost: i64,
    pub low: Located,
    pub high: Located,
}

pub struct Oracle {
    pub pools: Vec<Vec<Point>>,
    pub odd: bool,
    pub memory_mib: usize,
    pub queries: usize,
    pub rectangle_runs: usize,
    pub peak_bytes: usize,
    pub residues: usize,
    pub target: usize,
}

impl Oracle {
    pub fn new(rectangles: &[(i32, i32)], odd: bool, memory_mib: usize) -> Self {
        Self {
            pools: rectangles.iter().map(|&(a, b)| upper_pool(a, b)).collect(),
            odd,
            memory_mib,
            queries: 0,
            rectangle_runs: 0,
            peak_bytes: 0,
            residues: if odd { 2 } else { 1 },
            target: usize::from(odd),
        }
    }

    pub fn evaluate(&mut self, penalty: Penalty) -> Result<Extremes, String> {
        self.queries += 1;
        let mut best: Option<Extremes> = None;
        for (rectangle, vectors) in self.pools.iter().enumerate() {
            let Some(cap) =
                crate::bounds::count_cap(vectors, penalty, self.target, self.residues, self.odd)
            else {
                continue;
            };
            let table = penalty::Table::build(
                vectors,
                penalty::Settings {
                    penalty,
                    residues: self.residues,
                    cap,
                    memory_mib: self.memory_mib,
                },
            )?;
            self.peak_bytes = self.peak_bytes.max(table.bytes());
            self.rectangle_runs += 1;
            let Ok(result) = penalty::join(&table, penalty, self.target, self.odd) else {
                continue;
            };
            let candidate = Extremes {
                cost: result.cost,
                low: Located {
                    winner: result.low,
                    rectangle,
                    penalty,
                    cap,
                },
                high: Located {
                    winner: result.high,
                    rectangle,
                    penalty,
                    cap,
                },
            };
            merge(&mut best, candidate);
        }
        best.ok_or_else(|| "empty rectangle family".into())
    }

    pub fn search(&mut self, k: usize) -> Result<Option<Located>, String> {
        self.target = k;
        let maximum = 2 * self.pools.iter().map(Vec::len).max().unwrap_or(0);
        if k > maximum {
            return Err("target exceeds the domain's vertex capacity".into());
        }
        loop {
            if let Some(found) = self.search_current(k)? {
                return Ok(Some(found));
            }
            self.residues *= 2;
            if self.residues > 2 * maximum {
                return Err("no polygon in the requested domain".into());
            }
        }
    }

    fn search_current(&mut self, k: usize) -> Result<Option<Located>, String> {
        let zero = self.evaluate(Penalty { num: 0, den: 1 })?;
        if let Some(winner) = hit(&zero, k) {
            return Ok(Some(winner));
        }
        if zero.high.winner.n > k {
            return Ok(None);
        }
        let mut low = zero.high;
        let limit = self.penalty_limit();
        let mut high_penalty = ((k * k / 10).max(1) as i64).min(limit);
        let mut high;
        loop {
            let result = self.evaluate(Penalty {
                num: high_penalty,
                den: 1,
            })?;
            if let Some(winner) = hit(&result, k) {
                return Ok(Some(winner));
            }
            if straddles(&result, k) {
                return Ok(None);
            }
            if result.high.winner.n > k {
                high = result.low;
                break;
            }
            if high_penalty == limit {
                return Ok(None);
            }
            low = result.high;
            high_penalty = (high_penalty + (high_penalty + 3) / 4).min(limit);
        }
        // Secants are a practical acceleration, capped to preserve the logarithmic bound.
        for _ in 0..8 {
            let penalty = Penalty {
                num: high.winner.area2 - low.winner.area2,
                den: (high.winner.n - low.winner.n) as i64,
            };
            let result = self.evaluate(penalty)?;
            if let Some(winner) = hit(&result, k) {
                return Ok(Some(winner));
            }
            if straddles(&result, k) {
                return Ok(None);
            }
            if result.high.winner.n < k {
                low = result.high;
            } else {
                high = result.low;
            }
        }
        self.binary_search(k, high_penalty)
    }

    fn penalty_limit(&self) -> i64 {
        self.pools
            .iter()
            .map(|vectors| {
                let mut sum = Point::default();
                let mut area = 0;
                for vector in vectors {
                    area += sum.det(*vector);
                    sum = sum.plus(*vector);
                }
                2 * area + 1
            })
            .max()
            .unwrap_or(1)
    }

    fn binary_search(&mut self, k: usize, high_penalty: i64) -> Result<Option<Located>, String> {
        let t = self.pools.iter().map(Vec::len).max().unwrap_or(1) as i64;
        let scale = 8 * t * t;
        let mut low = 0;
        let mut high = high_penalty
            .checked_mul(scale)
            .ok_or("penalty scale overflow")?;
        while high - low > 1 {
            let middle = low + (high - low) / 2;
            let result = self.evaluate(Penalty {
                num: middle,
                den: scale,
            })?;
            if let Some(winner) = hit(&result, k) {
                return Ok(Some(winner));
            }
            if straddles(&result, k) {
                return Ok(None);
            }
            if result.high.winner.n < k {
                low = middle;
            } else {
                high = middle;
            }
        }
        Ok(None)
    }
}

fn hit(result: &Extremes, k: usize) -> Option<Located> {
    if result.low.winner.n == k {
        return Some(result.low);
    }
    if result.high.winner.n == k {
        return Some(result.high);
    }
    None
}

fn straddles(result: &Extremes, k: usize) -> bool {
    result.low.winner.n < k && k < result.high.winner.n
}

fn merge(best: &mut Option<Extremes>, candidate: Extremes) {
    let Some(current) = best else {
        *best = Some(candidate);
        return;
    };
    if candidate.cost < current.cost {
        *current = candidate;
        return;
    }
    if candidate.cost != current.cost {
        return;
    }
    if candidate.low.winner.n < current.low.winner.n {
        current.low = candidate.low;
    }
    if candidate.high.winner.n > current.high.winner.n {
        current.high = candidate.high;
    }
}
