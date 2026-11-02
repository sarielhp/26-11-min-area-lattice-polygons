//! Count-major lattice-width DP: layers m = 2, 3, ..., K; only the source and
//! destination layers are resident. Joins use per-layer minimum-cost summaries.
use crate::Error;
use crate::frontier::Stats;
use crate::geom::{Point, validate, width_lower_bound};
use crate::layer::Layer;
use crate::store::Spill;
use crate::turns::TurnTable;
use std::time::Instant;

const EMPTY: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Join {
    Diagonal,
    OneFlat,
    TwoFlat,
}

#[derive(Clone, Copy, Debug)]
pub struct Solution {
    pub area2: i64,
    pub top: Point,
    pub right_count: usize,
    pub left_count: usize,
    pub join: Join,
}

pub struct Config {
    pub k: usize,
    pub threads: usize,
    /// upper[k]: valid feasible doubled areas (constructive default if absent).
    pub upper: Vec<i64>,
    /// lower[j]: certified lower bounds on B(j) (0 if unknown).
    pub lower: Vec<i64>,
    pub spill: Option<std::path::PathBuf>,
    /// Directory for certified polygon files of sizes >= `from` (requires `spill`).
    pub save: Option<std::path::PathBuf>,
    pub from: usize,
    /// Continue an interrupted run from the checkpoint in `spill`.
    pub resume: bool,
    /// Testing: fail right after checkpointing this layer, as a crash would.
    pub stop_after: Option<usize>,
    pub verbose: bool,
}

pub struct LayerReport {
    pub m: usize,
    pub entries: usize,
    pub cap: i64,
    pub h_limit: usize,
    pub seconds: f64,
}

pub struct Report {
    pub solutions: Vec<Option<Solution>>,
    pub polygons: Vec<Option<Vec<Point>>>,
    pub h_max: usize,
    pub stats: Stats,
    pub layers: Vec<LayerReport>,
    pub peak_layer_bytes: usize,
    pub summary_bytes: usize,
    pub spill_bytes: u64,
    pub total_entries: u64,
    pub search_seconds: f64,
    pub join_seconds: f64,
    pub turn_entries: usize,
}

/// Index of (h, x), h >= 1, 0 <= x < h, in a triangular summary array.
#[inline]
fn cell(h: usize, x: usize) -> usize {
    h * (h - 1) / 2 + x
}

/// Smallest searched height H with L_k(H+1) >= bound, so rows 1..=H certify k.
fn needed_height(k: usize, bound: i64) -> usize {
    let mut h = 1;
    while width_lower_bound(k, h as i64 + 1) < bound {
        h += 1;
    }
    h
}

struct Search<'a> {
    config: &'a Config,
    best: Vec<Option<Solution>>,
    summaries: Vec<Vec<u32>>,
    h_max: usize,
}

impl Search<'_> {
    fn bound(&self, k: usize) -> i64 {
        let found = self.best[k].map_or(i64::MAX, |s| s.area2);
        self.config.upper[k].min(found)
    }

    /// Rows needed by layer m: every k >= m that a chain with m vertices can serve.
    fn h_limit(&self, m: usize) -> usize {
        (m.max(3)..=self.config.k)
            .map(|k| needed_height(k, self.bound(k)))
            .max()
            .unwrap_or(0)
            .min(self.h_max)
    }

    /// A chain with m vertices and cost c inside a k-gon P leaves conv(O, p, rest),
    /// a strictly convex lattice (k-m+2)-gon, so area2(P) >= c + B(k-m+2).
    /// Sizes k < m are final when layer m is built, so B(j) is known for j < m.
    fn cap(&self, m: usize) -> i64 {
        let k_max = self.config.k;
        let mut lower = vec![0_i64; k_max + 3];
        for j in 3..lower.len() {
            let certified = if j < m {
                self.best[j].map_or(0, |s| s.area2)
            } else {
                0
            };
            let file = self.config.lower.get(j).copied().unwrap_or(0);
            lower[j] = lower[j - 1].max(certified).max(file);
        }
        (m.max(3)..=k_max)
            .map(|k| self.bound(k) - lower[k + 2 - m])
            .max()
            .unwrap_or(-1)
    }

    fn summarize(&self, layer: &Layer) -> Vec<u32> {
        let mut summary = vec![EMPTY; cell(self.h_max + 1, 0)];
        for (i, row) in layer.rows.iter().enumerate() {
            let h = layer.first_h + i;
            for x in 0..h {
                if let Some(first) = row.frontier(x).first() {
                    summary[cell(h, x)] = first.area;
                }
            }
        }
        summary
    }

    /// Joins the newest layer m with every layer n <= m, in both roles.
    fn join(&mut self, m: usize) {
        let k_max = self.config.k;
        let bounds: Vec<i64> = (0..=k_max).map(|k| if k < 3 { -1 } else { self.bound(k) }).collect();
        let mut pairs = Vec::new();
        for n in 2..=m {
            if m + n - 2 > k_max {
                continue;
            }
            pairs.push((m, n));
            if n != m {
                pairs.push((n, m));
            }
        }
        let summaries = &self.summaries;
        let h_max = self.h_max;
        let threads = self.config.threads.max(1);
        let work = |t: usize| {
            let mut local: Vec<Option<Solution>> = vec![None; k_max + 1];
            for h in (1..=h_max).filter(|h| h % threads == t) {
                for &(r, l) in &pairs {
                    join_height((summaries, h, r, l), &bounds, &mut local);
                }
            }
            local
        };
        let found: Vec<Vec<Option<Solution>>> = if threads == 1 {
            vec![work(0)]
        } else {
            std::thread::scope(|scope| {
                let work = &work;
                let workers: Vec<_> = (0..threads).map(|t| scope.spawn(move || work(t))).collect();
                workers.into_iter().map(|w| w.join().unwrap()).collect()
            })
        };
        for local in found {
            for (k, candidate) in local.into_iter().enumerate() {
                if let Some(c) = candidate
                    && self.best[k].is_none_or(|old| c.area2 < old.area2)
                {
                    self.best[k] = Some(c);
                }
            }
        }
    }
}

fn join_height(
    at: (&[Vec<u32>], usize, usize, usize),
    bounds: &[i64],
    local: &mut [Option<Solution>],
) {
    let (summaries, h, r, l) = at;
    let (right, left) = (&summaries[r], &summaries[l]);
    for x in 0..h {
        let right_area = right[cell(h, x)];
        if right_area == EMPTY {
            continue;
        }
        let opposite = if x == 0 { 0 } else { h - x };
        let left_x = [opposite, (h + 1 - x) % h, opposite];
        for (flats, join) in [Join::Diagonal, Join::OneFlat, Join::TwoFlat].into_iter().enumerate() {
            let k = r + l - 2 + flats;
            if k < 3 || k >= bounds.len() {
                continue;
            }
            let left_area = left[cell(h, left_x[flats])];
            if left_area == EMPTY {
                continue;
            }
            let area2 = i64::from(right_area) + i64::from(left_area) + (flats * h) as i64;
            if area2 > bounds[k] || local[k].is_some_and(|old| area2 >= old.area2) {
                continue;
            }
            local[k] = Some(Solution {
                area2,
                top: Point {
                    x: x as i64,
                    y: h as i64,
                },
                right_count: r,
                left_count: l,
                join,
            });
        }
    }
}

/// Reconstructs, validates, and optionally saves every size in next..=upto,
/// all of which are final. Returns the next size to emit.
fn emit(
    search: &Search,
    spill: &Spill,
    polygons: &mut [Option<Vec<Point>>],
    sizes: std::ops::RangeInclusive<usize>,
    elapsed: f64,
) -> Result<(), Error> {
    let config = search.config;
    for k in sizes {
        let solution = search.best[k].ok_or("missing solution")?;
        let points = polygon(spill, k, solution)?;
        if let Some(dir) = config.save.as_ref().filter(|_| k >= config.from) {
            let height = needed_height(k, solution.area2);
            let record = crate::save::Record {
                k,
                area2: solution.area2,
                points: &points,
                height,
                next_width_lower_bound: width_lower_bound(k, height as i64 + 1),
                threads: config.threads,
                run_target: config.k,
                elapsed,
            };
            if crate::save::write(dir, &record)? {
                eprintln!(
                    "k={k} certified A070911={} t={elapsed:.2}s -> {}",
                    solution.area2,
                    crate::save::path(dir, k).display()
                );
            }
        }
        polygons[k] = Some(points);
    }
    Ok(())
}

pub fn run(config: &Config) -> Result<Report, Error> {
    let started = Instant::now();
    let k_max = config.k;
    let mut search = Search {
        config,
        best: vec![None; k_max + 1],
        summaries: vec![Vec::new(); k_max + 1],
        h_max: 0,
    };
    search.h_max = (3..=k_max)
        .map(|k| needed_height(k, config.upper[k]))
        .max()
        .unwrap_or(1);
    let turns = TurnTable::new(search.h_max);
    let fingerprint = crate::checkpoint::Fingerprint {
        k: k_max,
        h_max: search.h_max,
        upper: config.upper.clone(),
        lower: config.lower.clone(),
    };
    let mut stats = Stats::default();
    let mut layers = Vec::new();
    let (mut peak, mut total_entries, mut join_seconds) = (0, 0_u64, 0.0);
    let mut polygons = vec![None; k_max + 1];
    let mut emitted = 2;

    // A resumed run starts with its last completed layer already processed.
    let (mut spill, mut layer, mut processed) = if config.resume {
        let dir = config.spill.clone().ok_or("--resume needs --spill DIR")?;
        let state = crate::checkpoint::load(&dir)?;
        if state.fingerprint != fingerprint {
            return Err("checkpoint was written with different --k, --upper, or --lower".into());
        }
        let spill = Spill::reopen(dir, state.last_layer)?;
        for m in 2..=state.last_layer {
            search.summaries[m] = spill.read_summary(m)?;
        }
        search.best = state.best;
        stats = state.stats;
        total_entries = state.total_entries;
        peak = state.peak_layer_bytes;
        let layer = spill.load_layer(state.last_layer)?;
        eprintln!("resuming after layer m={}", state.last_layer);
        (Some(spill), layer, true)
    } else {
        let spill = config.spill.clone().map(Spill::new).transpose()?;
        (spill, Layer::initial(search.h_limit(2))?, false)
    };
    loop {
        let m = layer.m;
        if !processed {
            total_entries += layer.entries() as u64;
            search.summaries[m] = search.summarize(&layer);
            if let Some(spill) = spill.as_mut() {
                spill.write(&layer)?;
                spill.write_summary(m, &search.summaries[m])?;
            }
            let join_started = Instant::now();
            search.join(m);
            join_seconds += join_started.elapsed().as_secs_f64();
        }
        if let Some(k) = (3..=m.min(k_max)).find(|&k| search.best[k].is_none()) {
            return Err(format!("no {k}-gon within the supplied upper bound; invalid --upper?").into());
        }
        // Every pair of counts <= m has been joined: sizes k <= m are final.
        if let Some(spill) = spill.as_ref() {
            let upto = m.min(k_max);
            emit(&search, spill, &mut polygons, emitted + 1..=upto, started.elapsed().as_secs_f64())?;
            emitted = emitted.max(upto);
            if !processed {
                crate::checkpoint::save(
                    spill.dir(),
                    &crate::checkpoint::State {
                        fingerprint: crate::checkpoint::Fingerprint {
                            upper: fingerprint.upper.clone(),
                            lower: fingerprint.lower.clone(),
                            ..fingerprint
                        },
                        last_layer: m,
                        best: search.best.clone(),
                        stats,
                        total_entries,
                        peak_layer_bytes: peak,
                    },
                )?;
                if config.stop_after == Some(m) {
                    return Err(format!("stopped after layer m={m} as requested").into());
                }
            }
        }
        processed = false;
        if config.verbose {
            eprintln!(
                "m={m} entries={} heights={}..{} t={:.2}s",
                layer.entries(),
                layer.first_h,
                layer.end_h(),
                started.elapsed().as_secs_f64()
            );
        }
        if m >= k_max || layer.entries() == 0 {
            break;
        }
        let layer_started = Instant::now();
        let (h_limit, cap) = (search.h_limit(m + 1), search.cap(m + 1));
        let (next, layer_stats) = layer.next(h_limit, cap, &turns, config.threads)?;
        peak = peak.max(layer.bytes() + next.bytes());
        stats.add(layer_stats);
        layers.push(LayerReport {
            m: m + 1,
            entries: next.entries(),
            cap,
            h_limit,
            seconds: layer_started.elapsed().as_secs_f64(),
        });
        layer = next;
    }
    drop(layer);
    if let Some(k) = (3..=k_max).find(|&k| search.best[k].is_none()) {
        return Err(format!("no {k}-gon within the supplied upper bound; invalid --upper?").into());
    }
    let search_seconds = started.elapsed().as_secs_f64();

    if let Some(spill) = spill.as_ref() {
        emit(&search, spill, &mut polygons, emitted + 1..=k_max, started.elapsed().as_secs_f64())?;
    }
    let spill_bytes = spill.as_ref().map_or(0, |s| s.bytes);
    if let Some(spill) = spill {
        spill.remove()?;
    }
    Ok(Report {
        summary_bytes: search.summaries.iter().map(|s| s.capacity() * 4).sum(),
        solutions: search.best,
        polygons,
        h_max: search.h_max,
        stats,
        layers,
        peak_layer_bytes: peak,
        spill_bytes,
        total_entries,
        search_seconds,
        join_seconds,
        turn_entries: turns.entries,
    })
}

fn chain(spill: &Spill, mut p: Point, mut count: usize, mut shear: i64) -> Result<Vec<Point>, Error> {
    let mut entry = *spill
        .frontier(count, p.y as usize, p.x as usize)?
        .first()
        .ok_or("missing chain")?;
    let mut points = Vec::with_capacity(count - 1);
    loop {
        points.push(Point {
            x: p.x + shear * p.y,
            y: p.y,
        });
        if count == 2 {
            break;
        }
        let q = p.minus(entry.incoming());
        if q.y <= 0 {
            return Err("invalid predecessor height".into());
        }
        let area2 = i64::try_from(i128::from(entry.area2()) - q.cross(p))?;
        shear += q.x.div_euclid(q.y);
        p = Point {
            x: q.x.rem_euclid(q.y),
            y: q.y,
        };
        count -= 1;
        let entries = spill.frontier(count, p.y as usize, p.x as usize)?;
        let index = entries
            .binary_search_by_key(&area2, |old| old.area2())
            .map_err(|_| "missing predecessor cost")?;
        entry = entries[index];
    }
    Ok(points)
}

fn polygon(spill: &Spill, k: usize, solution: Solution) -> Result<Vec<Point>, Error> {
    let mut right = chain(spill, solution.top, solution.right_count, 0)?;
    right.reverse();
    let top_x = solution.top.x - i64::from(solution.join == Join::OneFlat);
    let left_x = (-top_x).rem_euclid(solution.top.y);
    let shear = (-top_x - left_x) / solution.top.y;
    let left = chain(
        spill,
        Point {
            x: left_x,
            y: solution.top.y,
        },
        solution.left_count,
        shear,
    )?;
    let shift = i64::from(solution.join == Join::TwoFlat);
    let mut points = vec![Point { x: 0, y: 0 }];
    points.extend(right);
    points.extend(
        left.into_iter()
            .skip(usize::from(solution.join == Join::Diagonal))
            .map(|p| Point {
                x: -p.x - shift,
                y: p.y,
            }),
    );
    if solution.join == Join::TwoFlat {
        points.push(Point { x: -1, y: 0 });
    }
    validate(&points, k, solution.area2)?;
    Ok(points)
}
