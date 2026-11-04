//! Count-major evaluation of the heuristic pool DP, for every count that
//! uses one box side at once.
//!
//! `D_c(g)` after vector `j` is the minimum ordered determinant sum of a
//! `c`-subset of `v_0..=v_j` with endpoint sum `g` (the classic table). The
//! classic solver keeps every count's table while it sweeps the vectors.
//! Here the counts are the outer loop: layer `c` is swept over `j` using only
//! layer `c-1`, so two dense tables suffice. The history of layer `c-1` is
//! replayed from its improvement stream: every time `D_{c-1}(g)` decreases at
//! step `j`, the record `(g, cost)` is appended to that layer's stream under
//! step `j`. Streams are written and read sequentially (files in a spill
//! directory) and are kept for witness reconstruction, which scans them once,
//! from the top layer down, answering the predecessor lookups of all counts
//! together.
//!
//! Joins: even `k = 2c` uses `2 min_g D_c(g)` (centrally symmetric); odd
//! `k = 2c - 1` uses `min_g D_{c-1}(g) + D_c(g)` (the balanced split).
use crate::geometry::Point;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

type Error = Box<dyn std::error::Error + Send + Sync>;

const INF: u32 = u32::MAX;

#[derive(Clone, Copy)]
struct Grid {
    xmin: i32,
    ymin: i32,
    w: usize,
    h: usize,
}

impl Grid {
    /// Tight per-coordinate bounding box of the sums of exactly `c` vectors.
    fn of_count(pool: &[Point], c: usize) -> Self {
        let mut xs: Vec<i32> = pool.iter().map(|p| p.x).collect();
        let mut ys: Vec<i32> = pool.iter().map(|p| p.y).collect();
        xs.sort_unstable();
        ys.sort_unstable();
        let n = xs.len();
        let (xlo, xhi): (i32, i32) = (xs[..c].iter().sum(), xs[n - c..].iter().sum());
        let (ylo, yhi): (i32, i32) = (ys[..c].iter().sum(), ys[n - c..].iter().sum());
        Self {
            xmin: xlo,
            ymin: ylo,
            w: (xhi - xlo + 1) as usize,
            h: (yhi - ylo + 1) as usize,
        }
    }

    fn cells(self) -> usize {
        self.w * self.h
    }

    fn index(self, p: Point) -> Option<usize> {
        let dx = p.x - self.xmin;
        let dy = p.y - self.ymin;
        (dx >= 0 && dy >= 0 && (dx as usize) < self.w && (dy as usize) < self.h)
            .then(|| dy as usize * self.w + dx as usize)
    }

    fn point(self, i: usize) -> Point {
        Point {
            x: self.xmin + (i % self.w) as i32,
            y: self.ymin + (i / self.w) as i32,
        }
    }
}

/// Improvement records of one layer: `start[j]..start[j + 1]` are the
/// records of step `j` (vector `j`), stored in `path` as `(cell, cost)` pairs.
struct Stream {
    grid: Grid,
    start: Vec<u64>,
    path: PathBuf,
}

struct StreamReader {
    input: BufReader<File>,
}

impl StreamReader {
    fn open(stream: &Stream) -> Result<Self, Error> {
        Ok(Self {
            input: BufReader::with_capacity(1 << 22, File::open(&stream.path)?),
        })
    }

    fn next(&mut self) -> Result<(u32, u32), Error> {
        let mut b = [0_u8; 8];
        self.input.read_exact(&mut b)?;
        Ok((
            u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
        ))
    }
}

pub struct CountResult {
    pub k: usize,
    pub area2: i64,
    pub vertices: Vec<Point>,
}

pub struct Report {
    pub results: Vec<CountResult>,
    pub peak_table_bytes: usize,
    pub stream_bytes: u64,
    pub records: u64,
}

/// One step of layer `c`: relax every finite cell of `prev` by vector `v`
/// into `cur`, returning the improved cells and costs (rows in order).
#[allow(clippy::needless_range_loop)] // columns index both source and target rows
fn relax(
    prev: &[u32],
    pg: Grid,
    cur: &mut [u32],
    cg: Grid,
    v: Point,
) -> Result<Vec<(u32, u32)>, String> {
    // Target row ty corresponds to source row ty - v.y.
    let row_events: Vec<Result<Vec<(u32, u32)>, String>> = cur
        .par_chunks_mut(cg.w)
        .enumerate()
        .map(|(tr, target)| {
            let ty = cg.ymin + tr as i32;
            let y = ty - v.y;
            let mut events = Vec::new();
            if y < pg.ymin || y >= pg.ymin + pg.h as i32 {
                return Ok(events);
            }
            let sr = (y - pg.ymin) as usize;
            let source = &prev[sr * pg.w..(sr + 1) * pg.w];
            // Target x = source x + v.x; restrict to the overlap.
            let shift = pg.xmin + v.x - cg.xmin; // target column of source column 0
            let lo = (-shift).max(0) as usize;
            let hi = (pg.w as i64).min(cg.w as i64 - i64::from(shift));
            if hi <= lo as i64 {
                return Ok(events);
            }
            // cost = src + x*v.y - y*v.x, with x = pg.xmin + col.
            let mut add =
                i64::from(pg.xmin + lo as i32) * i64::from(v.y) - i64::from(y) * i64::from(v.x);
            let step = i64::from(v.y);
            for col in lo..hi as usize {
                let src = source[col];
                if src != INF {
                    let cost = i64::from(src) + add;
                    let tc = (col as i64 + i64::from(shift)) as usize;
                    if cost < i64::from(target[tc]) {
                        let cost =
                            u32::try_from(cost).map_err(|_| "cost outside u32".to_string())?;
                        target[tc] = cost;
                        events.push(((tr * cg.w + tc) as u32, cost));
                    }
                }
                add += step;
            }
            Ok(events)
        })
        .collect();
    let mut all = Vec::new();
    for events in row_events {
        all.extend(events?);
    }
    Ok(all)
}

/// Computes the box optimum and a witness for every `k` in `ks` (all with the
/// same pool). Streams are written to `spill` and removed at the end.
#[allow(clippy::needless_range_loop)] // the step index selects both the vector and the stream offsets
pub fn run(pool: &[Point], ks: &[usize], spill: &Path) -> Result<Report, Error> {
    let t = pool.len();
    let max_count = ks
        .iter()
        .map(|k| k.div_ceil(2))
        .max()
        .ok_or("no counts")?
        .min(t);
    std::fs::create_dir_all(spill)?;
    // best[k] = (area2, endpoint cell in layer ceil(k/2), cost of the (k-1)/2 chain for odd k)
    let mut best: HashMap<usize, (i64, usize, u32)> = HashMap::new();
    let mut streams: Vec<Stream> = Vec::new(); // streams[c - 1] = layer c
    let mut prev_grid = Grid {
        xmin: 0,
        ymin: 0,
        w: 1,
        h: 1,
    };
    let mut prev = vec![0_u32]; // layer 0: the empty subset, final from the start
    let (mut peak_table_bytes, mut stream_bytes, mut records) = (0, 0_u64, 0_u64);
    for c in 1..=max_count {
        let grid = Grid::of_count(pool, c);
        let mut cur = vec![INF; grid.cells()];
        // Rebuild prev from layer c-1's stream as the sweep advances.
        if c > 1 {
            prev = vec![INF; prev_grid.cells()];
        }
        let path = spill.join(format!("layer_{c:04}.bin"));
        // Reader thread: prefetches layer c-1's records step by step.
        // Writer thread: writes layer c's records while the next step runs.
        let (start, written) = std::thread::scope(|scope| -> Result<(Vec<u64>, u64), Error> {
            let (read_tx, read_rx) =
                std::sync::mpsc::sync_channel::<Result<Vec<(u32, u32)>, String>>(4);
            if c > 1 {
                let s = &streams[c - 2];
                scope.spawn(move || {
                    let mut reader = match StreamReader::open(s) {
                        Ok(r) => r,
                        Err(e) => {
                            let _ = read_tx.send(Err(e.to_string()));
                            return;
                        }
                    };
                    for j in 0..t {
                        let n = (s.start[j + 1] - s.start[j]) as usize;
                        let mut batch = Vec::with_capacity(n);
                        for _ in 0..n {
                            match reader.next() {
                                Ok(rec) => batch.push(rec),
                                Err(e) => {
                                    let _ = read_tx.send(Err(e.to_string()));
                                    return;
                                }
                            }
                        }
                        if read_tx.send(Ok(batch)).is_err() {
                            return;
                        }
                    }
                });
            } else {
                drop(read_tx);
            }
            let (write_tx, write_rx) = std::sync::mpsc::sync_channel::<Vec<(u32, u32)>>(4);
            let path_ref = &path;
            let writer = scope.spawn(move || -> Result<(), String> {
                let file = File::create(path_ref).map_err(|e| e.to_string())?;
                let mut out = BufWriter::with_capacity(1 << 22, file);
                let mut bytes = Vec::new();
                for events in write_rx {
                    bytes.clear();
                    for &(cell, cost) in &events {
                        bytes.extend_from_slice(&cell.to_le_bytes());
                        bytes.extend_from_slice(&cost.to_le_bytes());
                    }
                    out.write_all(&bytes).map_err(|e| e.to_string())?;
                }
                out.flush().map_err(|e| e.to_string())
            });
            let mut start = Vec::with_capacity(t + 1);
            let mut written = 0_u64;
            for j in 0..=t {
                // Layer c-1 records of step j-1 become usable at step j.
                if c > 1 && j > 0 {
                    let batch = read_rx.recv().map_err(|_| "reader thread ended early")??;
                    for (cell, cost) in batch {
                        prev[cell as usize] = cost;
                    }
                }
                start.push(written);
                if j == t {
                    break;
                }
                let events = relax(&prev, prev_grid, &mut cur, grid, pool[j])?;
                written += events.len() as u64;
                write_tx
                    .send(events)
                    .map_err(|_| "writer thread ended early")?;
            }
            drop(write_tx);
            writer.join().map_err(|_| "writer thread panicked")??;
            Ok((start, written))
        })?;
        records += written;
        stream_bytes += 8 * written;
        peak_table_bytes = peak_table_bytes.max(4 * (prev.len() + cur.len()));
        // Joins.
        let even = 2 * c;
        if ks.contains(&even)
            && let Some((i, &a)) = cur
                .iter()
                .enumerate()
                .filter(|(_, a)| **a != INF)
                .min_by_key(|(_, a)| **a)
        {
            best.insert(even, (2 * i64::from(a), i, a));
        }
        let odd = 2 * c - 1;
        if odd >= 3 && ks.contains(&odd) {
            let mut b: Option<(i64, usize, u32)> = None;
            for (i, &a) in cur.iter().enumerate() {
                if a == INF {
                    continue;
                }
                if let Some(pi) = prev_grid.index(grid.point(i))
                    && prev[pi] != INF
                {
                    let s = i64::from(prev[pi]) + i64::from(a);
                    if b.is_none_or(|(old, _, _)| s < old) {
                        b = Some((s, i, prev[pi]));
                    }
                }
            }
            if let Some(b) = b {
                best.insert(odd, b);
            }
        }
        streams.push(Stream { grid, start, path });
        prev_grid = grid;
        prev = cur;
    }
    let results = witnesses(pool, ks, &streams, &best)?;
    for s in &streams {
        std::fs::remove_file(&s.path)?;
    }
    Ok(Report {
        results,
        peak_table_bytes,
        stream_bytes,
        records,
    })
}

/// A pending predecessor lookup: chain `chain` needs the record of layer `c`
/// at `cell` with exactly `cost`.
struct Lookup {
    chain: usize,
    cell: u32,
    cost: u32,
}

/// Backtracks all requested chains in one pass over the streams, top layer
/// first. Each record matched gives the chain's last vector and the lookup
/// for the layer below.
fn witnesses(
    pool: &[Point],
    ks: &[usize],
    streams: &[Stream],
    best: &HashMap<usize, (i64, usize, u32)>,
) -> Result<Vec<CountResult>, Error> {
    // Chains: (layer, end cell, cost). Even k: one chain of k/2 used twice.
    // Odd k: chains of (k-1)/2 and (k+1)/2 vectors with the same sum.
    let mut chains: Vec<(usize, Point, u32)> = Vec::new();
    let mut plan: Vec<(usize, i64, usize, usize)> = Vec::new(); // k, area, chain a, chain b
    for &k in ks {
        let Some(&(area2, cell, low_cost)) = best.get(&k) else {
            continue;
        };
        let c_hi = k.div_ceil(2);
        let end = streams[c_hi - 1].grid.point(cell);
        let id = chains.len();
        if k % 2 == 0 {
            chains.push((c_hi, end, low_cost));
            plan.push((k, area2, id, id));
        } else {
            let high_cost = u32::try_from(area2 - i64::from(low_cost))?;
            chains.push((c_hi - 1, end, low_cost));
            chains.push((c_hi, end, high_cost));
            plan.push((k, area2, id, id + 1));
        }
    }
    let mut edges: Vec<Vec<Point>> = vec![Vec::new(); chains.len()];
    let mut pending: HashMap<usize, Vec<Lookup>> = HashMap::new();
    for (id, &(c, p, cost)) in chains.iter().enumerate() {
        if c == 0 {
            continue;
        }
        let cell = streams[c - 1]
            .grid
            .index(p)
            .ok_or("chain end outside grid")? as u32;
        pending.entry(c).or_default().push(Lookup {
            chain: id,
            cell,
            cost,
        });
    }
    let top = chains.iter().map(|c| c.0).max().unwrap_or(0);
    for c in (1..=top).rev() {
        let Some(lookups) = pending.remove(&c) else {
            continue;
        };
        let s = &streams[c - 1];
        let mut want: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
        for l in &lookups {
            want.entry((l.cell, l.cost)).or_default().push(l.chain);
        }
        let mut found = 0;
        let mut reader = StreamReader::open(s)?;
        let mut step = 0;
        for r in 0..*s.start.last().unwrap() {
            while s.start[step + 1] <= r {
                step += 1;
            }
            let rec = reader.next()?;
            let Some(ids) = want.remove(&rec) else {
                continue;
            };
            found += ids.len();
            let v = pool[step];
            let g = s.grid.point(rec.0 as usize);
            let q = g.minus(v);
            let pred_cost = i64::from(rec.1) - i64::from(q.x) * i64::from(v.y)
                + i64::from(q.y) * i64::from(v.x);
            for id in ids {
                edges[id].push(v);
                if c > 1 {
                    let pg = streams[c - 2].grid;
                    let cell = pg.index(q).ok_or("predecessor outside grid")? as u32;
                    pending.entry(c - 1).or_default().push(Lookup {
                        chain: id,
                        cell,
                        cost: u32::try_from(pred_cost)?,
                    });
                } else if q != Point::default() || pred_cost != 0 {
                    return Err("chain does not start at the origin".into());
                }
            }
            if want.is_empty() {
                break;
            }
        }
        if found != lookups.len() {
            return Err(format!(
                "layer {c}: {} predecessor records not found",
                lookups.len() - found
            )
            .into());
        }
    }
    let mut results = Vec::new();
    for (k, area2, a, b) in plan {
        // Chains were built from the top vector down; reverse to angular order.
        let mut left: Vec<Point> = edges[a].iter().rev().copied().collect();
        let right: Vec<Point> = edges[b].iter().rev().copied().collect();
        left.extend(right.into_iter().map(Point::negative));
        let vertices = crate::geometry::validate_polygon(&left, area2)?;
        if vertices.len() != k {
            return Err(format!("k={k}: witness has {} vertices", vertices.len()).into());
        }
        results.push(CountResult { k, area2, vertices });
    }
    Ok(results)
}
