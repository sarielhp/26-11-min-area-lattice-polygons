use crate::geometry::Point;

const INF: i64 = i64::MAX / 4;

#[derive(Clone, Copy, Debug)]
pub struct Penalty {
    pub num: i64,
    pub den: i64,
}

#[derive(Clone, Copy)]
pub struct Settings {
    pub penalty: Penalty,
    pub residues: usize,
    pub cap: usize,
    pub memory_mib: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub cost: i64,
    pub low: u32,
    pub high: u32,
}

impl Cell {
    fn empty() -> Self {
        Self {
            cost: INF,
            low: 0,
            high: 0,
        }
    }

    fn relax(&mut self, candidate: Self) {
        if candidate.cost < self.cost {
            *self = candidate;
        } else if candidate.cost == self.cost {
            self.low = self.low.min(candidate.low);
            self.high = self.high.max(candidate.high);
        }
    }

    pub fn finite(self) -> bool {
        self.cost != INF
    }

    pub fn count(self, maximum: bool) -> usize {
        if maximum {
            self.high as usize
        } else {
            self.low as usize
        }
    }
}

pub struct Table {
    pub xmin: i32,
    pub xmax: i32,
    pub ymax: i32,
    pub parities: usize,
    pub cells: Vec<Cell>,
}

impl Table {
    fn width(&self) -> usize {
        (self.xmax - self.xmin + 1) as usize
    }

    fn index(&self, point: Point, parity: usize) -> usize {
        ((point.y as usize * self.width()) + (point.x - self.xmin) as usize) * self.parities
            + parity
    }

    pub fn get(&self, point: Point, parity: usize) -> Cell {
        if point.x < self.xmin || point.x > self.xmax || point.y < 0 || point.y > self.ymax {
            return Cell::empty();
        }
        self.cells[self.index(point, parity)]
    }

    pub fn bytes(&self) -> usize {
        self.cells.len() * std::mem::size_of::<Cell>()
    }

    pub fn build(vectors: &[Point], settings: Settings) -> Result<Self, String> {
        let Settings {
            penalty,
            residues: parities,
            cap,
            memory_mib,
        } = settings;
        let mut xs: Vec<_> = vectors.iter().map(|v| v.x).collect();
        xs.sort_unstable();
        let xmin = sum_coordinates(xs.iter().take(cap).filter(|x| **x < 0))?;
        let xmax = sum_coordinates(xs.iter().rev().take(cap).filter(|x| **x > 0))?;
        let mut ys: Vec<_> = vectors.iter().map(|v| v.y).collect();
        ys.sort_unstable();
        let ymax = sum_coordinates(ys.iter().rev().take(cap))?;
        let width = i64::from(xmax) - i64::from(xmin) + 1;
        if width > i64::from(i32::MAX) || ymax == i32::MAX {
            return Err("endpoint range exceeds checked 32-bit limits".into());
        }
        let length = (width as usize)
            .checked_mul(ymax as usize + 1)
            .and_then(|size| size.checked_mul(parities))
            .ok_or("table size overflow")?;
        let bytes = length
            .checked_mul(std::mem::size_of::<Cell>())
            .ok_or("table size overflow")?;
        if bytes > memory_mib * 1048576 {
            return Err(format!(
                "penalty table requires {:.1} MiB",
                bytes as f64 / 1048576.0
            ));
        }
        check_cost_range(vectors, penalty)?;
        let mut table = Self {
            xmin,
            xmax,
            ymax,
            parities,
            cells: vec![Cell::empty(); length],
        };
        let origin = table.index(Point::default(), 0);
        table.cells[origin] = Cell {
            cost: 0,
            low: 0,
            high: 0,
        };
        let mut active = (0, 0, 0);
        for vector in vectors {
            table.extend(*vector, penalty, active);
            active = (
                active.0.saturating_add(vector.x.min(0)).max(xmin),
                active.1.saturating_add(vector.x.max(0)).min(xmax),
                active.2.saturating_add(vector.y).min(ymax),
            );
        }
        Ok(table)
    }

    fn extend(&mut self, vector: Point, penalty: Penalty, active: (i32, i32, i32)) {
        if vector.y == 0 {
            // The only horizontal candidate is (1,0), first in angular order.
            let target = self.index(vector, 1 % self.parities);
            self.cells[target].relax(Cell {
                cost: -penalty.num,
                low: 1,
                high: 1,
            });
            return;
        }
        let left = active.0.max(self.xmin - vector.x);
        let right = active.1.min(self.xmax - vector.x);
        let top = active.2.min(self.ymax - vector.y);
        if left > right {
            return;
        }
        let length = (right - left + 1) as usize * self.parities;
        for y in (0..=top).rev() {
            let point = Point { x: left, y };
            let start = self.index(point, 0);
            let dest = self.index(point.plus(vector), 0);
            let cost = (
                penalty.den * point.det(vector) - penalty.num,
                penalty.den * i64::from(vector.y),
            );
            // Positive y gives disjoint rows. Descending row order prevents reuse.
            let (before, after) = self.cells.split_at_mut(dest);
            update_row(
                &before[start..start + length],
                &mut after[..length],
                cost,
                self.parities,
            );
        }
    }
}

fn sum_coordinates<'a>(values: impl Iterator<Item = &'a i32>) -> Result<i32, String> {
    i32::try_from(values.map(|x| i64::from(*x)).sum::<i64>())
        .map_err(|_| "endpoint sum exceeds checked 32-bit limits".into())
}

fn update_row(source: &[Cell], target: &mut [Cell], cost: (i64, i64), parities: usize) {
    let (mut increment, step) = cost;
    if parities == 1 {
        for (&src, dst) in source.iter().zip(target) {
            extend_cell(dst, src, increment);
            increment += step;
        }
        return;
    }
    if parities == 2 {
        for (src, dst) in source
            .as_chunks::<2>()
            .0
            .iter()
            .zip(target.as_chunks_mut::<2>().0)
        {
            extend_cell(&mut dst[1], src[0], increment);
            extend_cell(&mut dst[0], src[1], increment);
            increment += step;
        }
        return;
    }
    for (src, dst) in source
        .chunks_exact(parities)
        .zip(target.chunks_exact_mut(parities))
    {
        for side in 0..parities - 1 {
            extend_cell(&mut dst[side + 1], src[side], increment);
        }
        extend_cell(&mut dst[0], src[parities - 1], increment);
        increment += step;
    }
}

fn extend_cell(target: &mut Cell, source: Cell, increment: i64) {
    if source.finite() {
        target.relax(Cell {
            cost: source.cost + increment,
            low: source.low + 1,
            high: source.high + 1,
        });
    }
}

fn check_cost_range(vectors: &[Point], penalty: Penalty) -> Result<(), String> {
    let max_x = vectors
        .iter()
        .map(|v| i128::from(v.x).abs())
        .max()
        .unwrap_or(0);
    let max_y = vectors.iter().map(|v| i128::from(v.y)).max().unwrap_or(0);
    let count = vectors.len() as i128;
    let bound = i128::from(penalty.den) * 2 * max_x * max_y * count * count
        + i128::from(penalty.num) * count;
    if penalty.num < 0 || penalty.den <= 0 || bound >= i128::from(INF) / 4 {
        return Err("penalty exceeds checked integer limits".into());
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
pub struct Winner {
    pub n: usize,
    pub area2: i64,
    pub sum: Point,
    pub maximum: bool,
    pub residue: usize,
}

#[derive(Clone, Copy)]
pub struct Extremes {
    pub cost: i64,
    pub low: Winner,
    pub high: Winner,
}

pub fn join(table: &Table, penalty: Penalty, k: usize, odd: bool) -> Result<Extremes, String> {
    let mut result: Option<Extremes> = None;
    for y in 0..=table.ymax {
        for x in table.xmin..=table.xmax {
            let point = Point { x, y };
            let sides = if odd {
                0..table.parities
            } else {
                (k / 2) % table.parities..(k / 2) % table.parities + 1
            };
            for residue in sides {
                let other = if odd {
                    (k % table.parities + table.parities - residue) % table.parities
                } else {
                    residue
                };
                if odd && residue > other {
                    continue;
                }
                let first = table.get(point, residue);
                let second = table.get(point, other);
                if !first.finite() || !second.finite() {
                    continue;
                }
                let cost = first.cost + second.cost;
                let low_n = (first.low + second.low) as usize;
                let high_n = (first.high + second.high) as usize;
                let low = Winner {
                    n: low_n,
                    area2: (cost + penalty.num * low_n as i64) / penalty.den,
                    sum: point,
                    maximum: false,
                    residue,
                };
                let high = Winner {
                    n: high_n,
                    area2: (cost + penalty.num * high_n as i64) / penalty.den,
                    sum: point,
                    maximum: true,
                    residue,
                };
                merge_extremes(&mut result, Extremes { cost, low, high });
            }
        }
    }
    result.ok_or_else(|| "no feasible parity join".into())
}

pub fn merge_extremes(result: &mut Option<Extremes>, candidate: Extremes) {
    let Some(current) = result else {
        *result = Some(candidate);
        return;
    };
    if candidate.cost < current.cost {
        *current = candidate;
        return;
    }
    if candidate.cost != current.cost {
        return;
    }
    if candidate.low.n < current.low.n {
        current.low = candidate.low;
    }
    if candidate.high.n > current.high.n {
        current.high = candidate.high;
    }
}

/// Return both angular halves using penalized tables only; no count-indexed DP.
pub fn recover(
    vectors: &[Point],
    target: Point,
    parity: usize,
    maximum: bool,
    settings: Settings,
) -> Result<Vec<Point>, String> {
    let Settings {
        penalty,
        residues,
        memory_mib,
        ..
    } = settings;
    if vectors.len() <= 1 {
        if target == Point::default() && parity == 0 {
            return Ok(vec![]);
        }
        if vectors.len() == 1 && target == vectors[0] && parity == 1 % residues {
            return Ok(vectors.to_vec());
        }
        return Err("invalid terminal witness".into());
    }
    let middle = vectors.len() / 2;
    let left = Table::build(&vectors[..middle], settings)?;
    let remaining = memory_mib.saturating_sub(left.bytes().div_ceil(1048576));
    let right = Table::build(
        &vectors[middle..],
        Settings {
            memory_mib: remaining,
            ..settings
        },
    )?;
    let split = find_split(&left, &right, target, parity, maximum, penalty)?;
    drop(left);
    drop(right);
    let mut result = recover(&vectors[..middle], split.0, split.1, maximum, settings)?;
    result.extend(recover(
        &vectors[middle..],
        target.minus(split.0),
        (parity + residues - split.1) % residues,
        maximum,
        settings,
    )?);
    Ok(result)
}

fn find_split(
    left: &Table,
    right: &Table,
    target: Point,
    parity: usize,
    maximum: bool,
    penalty: Penalty,
) -> Result<(Point, usize), String> {
    let mut best: Option<(i64, usize, Point, usize)> = None;
    for y in 0..=left.ymax {
        for x in left.xmin..=left.xmax {
            let point = Point { x, y };
            for side in 0..left.parities {
                let a = left.get(point, side);
                let b = right.get(
                    target.minus(point),
                    (parity + left.parities - side) % left.parities,
                );
                if !a.finite() || !b.finite() {
                    continue;
                }
                let cost = a.cost + b.cost + penalty.den * point.det(target);
                let count = a.count(maximum) + b.count(maximum);
                let improve = best.is_none_or(|old| {
                    cost < old.0
                        || (cost == old.0
                            && if maximum {
                                count > old.1
                            } else {
                                count < old.1
                            })
                });
                if improve {
                    best = Some((cost, count, point, side));
                }
            }
        }
    }
    best.map(|entry| (entry.2, entry.3))
        .ok_or_else(|| "no witness split".into())
}
