//! One vertex-count layer: every canonical endpoint (x, h), 0 <= x < h, of chains
//! with exactly m vertices. Layer m+1 depends only on layer m.
use crate::Error;
use crate::frontier::{Packed, Stats, insert};
use crate::geom::{Point, gcd};
use crate::turns::TurnTable;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
pub struct HeightRow {
    pub offsets: Vec<u32>,
    pub entries: Vec<Packed>,
}

impl HeightRow {
    #[inline]
    pub fn frontier(&self, x: usize) -> &[Packed] {
        &self.entries[self.offsets[x] as usize..self.offsets[x + 1] as usize]
    }

    fn from_frontiers(frontiers: &[Vec<Packed>]) -> Result<Self, Error> {
        let total: usize = frontiers.iter().map(Vec::len).sum();
        let mut offsets = Vec::with_capacity(frontiers.len() + 1);
        let mut entries = Vec::with_capacity(total);
        offsets.push(0);
        for frontier in frontiers {
            entries.extend_from_slice(frontier);
            offsets.push(u32::try_from(entries.len()).map_err(|_| "height row exceeds u32 entries")?);
        }
        Ok(Self { offsets, entries })
    }

    pub fn bytes(&self) -> usize {
        self.offsets.capacity() * 4 + self.entries.capacity() * size_of::<Packed>()
    }
}

pub struct Layer {
    pub m: usize,
    pub first_h: usize,
    // rows[h - first_h] for first_h <= h <= first_h + rows.len() - 1.
    pub rows: Vec<HeightRow>,
}

impl Layer {
    pub fn end_h(&self) -> usize {
        self.first_h + self.rows.len()
    }

    pub fn entries(&self) -> usize {
        self.rows.iter().map(|r| r.entries.len()).sum()
    }

    pub fn bytes(&self) -> usize {
        self.rows.iter().map(HeightRow::bytes).sum()
    }

    /// Two-vertex chains: primitive endpoints, zero cost.
    pub fn initial(h_limit: usize) -> Result<Self, Error> {
        let mut rows = Vec::with_capacity(h_limit);
        for h in 1..=h_limit {
            let frontiers: Vec<Vec<Packed>> = (0..h)
                .map(|x| {
                    let p = Point {
                        x: x as i64,
                        y: h as i64,
                    };
                    if gcd(p.x, p.y) == 1 {
                        Packed::new(p, 0).map(|e| vec![e])
                    } else {
                        Ok(Vec::new())
                    }
                })
                .collect::<Result<_, _>>()?;
            rows.push(HeightRow::from_frontiers(&frontiers)?);
        }
        Ok(Self {
            m: 2,
            first_h: 1,
            rows,
        })
    }

    /// Builds layer m+1 for heights m..=h_limit. `cap` bounds chain costs in the new layer.
    pub fn next(
        &self,
        h_limit: usize,
        cap: i64,
        turns: &TurnTable,
        threads: usize,
    ) -> Result<(Layer, Stats), Error> {
        if cap > i64::from(u32::MAX) {
            return Err("area cap exceeds packed u32 range".into());
        }
        let first_h = self.m.max(1);
        let heights = (h_limit + 1).saturating_sub(first_h);
        let results: Mutex<Vec<Option<HeightRow>>> =
            Mutex::new((0..heights).map(|_| None).collect());
        let next = AtomicUsize::new(0);
        let worker = || -> Result<Stats, Error> {
            let mut stats = Stats::default();
            let mut buffers: Vec<Vec<Packed>> = Vec::new();
            loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= heights {
                    return Ok(stats);
                }
                // Largest (most expensive) destination heights first.
                let height = h_limit - i;
                if buffers.len() < height {
                    buffers.resize_with(height, Vec::new);
                }
                let destinations = &mut buffers[..height];
                destinations.iter_mut().for_each(Vec::clear);
                self.fill_height(height, cap, turns, destinations, &mut stats)?;
                let row = HeightRow::from_frontiers(destinations)?;
                results.lock().unwrap()[height - first_h] = Some(row);
            }
        };
        // One thread runs on the caller: a fresh thread per layer lets the
        // scheduler move the work between cores (measured 2x slower on a hybrid CPU).
        let outcomes: Vec<Result<Stats, Error>> = if threads <= 1 {
            vec![worker()]
        } else {
            std::thread::scope(|scope| {
                let workers: Vec<_> = (0..threads).map(|_| scope.spawn(&worker)).collect();
                workers.into_iter().map(|w| w.join().unwrap()).collect()
            })
        };
        let mut stats = Stats::default();
        for outcome in outcomes {
            stats.add(outcome?);
        }
        let rows = results
            .into_inner()
            .unwrap()
            .into_iter()
            .map(|r| r.unwrap_or_default())
            .collect();
        Ok((
            Layer {
                m: self.m + 1,
                first_h,
                rows,
            },
            stats,
        ))
    }

    fn fill_height(
        &self,
        height: usize,
        cap: i64,
        turns: &TurnTable,
        destinations: &mut [Vec<Packed>],
        stats: &mut Stats,
    ) -> Result<(), Error> {
        for source_h in self.first_h..height.min(self.end_h()) {
            let row = &self.rows[source_h - self.first_h];
            for x in 0..source_h {
                let frontier = row.frontier(x);
                if frontier.first().is_none_or(|e| e.area2() > cap) {
                    continue;
                }
                let origin = Point {
                    x: x as i64,
                    y: source_h as i64,
                };
                propagate(frontier, origin, height as i64, cap, turns, destinations, stats)?;
            }
        }
        Ok(())
    }
}

#[inline]
fn propagate(
    frontier: &[Packed],
    origin: Point,
    height: i64,
    cap: i64,
    turns: &TurnTable,
    destinations: &mut [Vec<Packed>],
    stats: &mut Stats,
) -> Result<(), Error> {
    let dy = height - origin.y;
    for packed in frontier {
        let area2 = packed.area2();
        if area2 > cap {
            break;
        }
        // 32-bit division: |x| < 2^15, 0 < y < 2^16.
        let (fx, fy) = (i32::from(packed.x), i32::from(packed.y));
        let quotient = fx.div_euclid(fy);
        let residue = i64::from(fx - quotient * fy);
        let shift_x = i64::from(quotient) * dy;
        let base = area2 + origin.x * dy;
        // outgoing_x increases along the list, so the area decreases: cheapest first.
        for turn in turns.turns(i64::from(fy), residue, dy).iter().rev() {
            let outgoing_x = turn.outgoing_x + shift_x;
            let raw_x = origin.x + outgoing_x;
            if raw_x >= height {
                continue;
            }
            // Area is shear-invariant.
            let area = base - origin.y * outgoing_x;
            if area > cap {
                break;
            }
            // Canonical shear -floor(raw_x / height) >= 0, computed in 32 bits.
            let shear = if raw_x >= 0 {
                0
            } else {
                -i64::from(i32::try_from(raw_x)?.div_euclid(height as i32))
            };
            let x = (raw_x + shear * height) as usize;
            let edge = Point {
                x: outgoing_x + shear * dy,
                y: dy,
            };
            let area = u32::try_from(area).map_err(|_| "negative or oversized chain area")?;
            insert(&mut destinations[x], edge, area, stats)?;
        }
    }
    Ok(())
}
