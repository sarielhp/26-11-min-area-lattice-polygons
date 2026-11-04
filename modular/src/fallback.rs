use crate::geometry::Point;

pub const INF: i32 = i32::MAX;

#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub xmin: i32,
    pub xmax: i32,
    pub ymin: i32,
    pub ymax: i32,
}

impl Bounds {
    fn origin() -> Self {
        Self {
            xmin: 0,
            xmax: 0,
            ymin: 0,
            ymax: 0,
        }
    }

    fn shifted(self, v: Point) -> Self {
        Self {
            xmin: self.xmin + v.x,
            xmax: self.xmax + v.x,
            ymin: self.ymin + v.y,
            ymax: self.ymax + v.y,
        }
    }

    fn union(self, other: Self) -> Self {
        Self {
            xmin: self.xmin.min(other.xmin),
            xmax: self.xmax.max(other.xmax),
            ymin: self.ymin.min(other.ymin),
            ymax: self.ymax.max(other.ymax),
        }
    }

    fn intersection(self, other: Self) -> Self {
        Self {
            xmin: self.xmin.max(other.xmin),
            xmax: self.xmax.min(other.xmax),
            ymin: self.ymin.max(other.ymin),
            ymax: self.ymax.min(other.ymax),
        }
    }

    fn width(self) -> usize {
        (self.xmax - self.xmin + 1) as usize
    }

    fn cells(self) -> usize {
        self.width() * (self.ymax - self.ymin + 1) as usize
    }

    fn contains(self, p: Point) -> bool {
        self.xmin <= p.x && p.x <= self.xmax && self.ymin <= p.y && p.y <= self.ymax
    }
}

fn extend_bounds(bounds: &mut [Option<Bounds>], v: Point, max_count: usize) {
    for count in (1..=max_count).rev() {
        let Some(previous) = bounds[count - 1] else {
            continue;
        };
        let added = previous.shifted(v);
        bounds[count] = Some(bounds[count].map_or(added, |old| old.union(added)));
    }
}

pub struct Table {
    pub bounds: Bounds,
    pub costs: Vec<i32>,
}

impl Table {
    fn index(&self, p: Point) -> usize {
        (p.y - self.bounds.ymin) as usize * self.bounds.width() + (p.x - self.bounds.xmin) as usize
    }

    pub fn get(&self, p: Point) -> i32 {
        if !self.bounds.contains(p) {
            return INF;
        }
        self.costs[self.index(p)]
    }
}

pub struct Dp {
    pub tables: Vec<Table>,
    pub bytes: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct Winner {
    pub area2: i64,
    pub count: usize,
    pub sum: Point,
}

impl Dp {
    pub fn build(vectors: &[Point], max_count: usize, memory_mib: usize) -> Result<Self, String> {
        let max_count = max_count.min(vectors.len());
        let max_x = vectors
            .iter()
            .map(|v| i64::from(v.x).abs())
            .max()
            .unwrap_or(0);
        let max_y = vectors
            .iter()
            .map(|v| i64::from(v.y).abs())
            .max()
            .unwrap_or(0);
        let cost_bound =
            2_i128 * i128::from(max_x) * i128::from(max_y) * (max_count as i128).pow(2);
        if cost_bound >= i128::from(INF) || max_count as i64 * max_x.max(max_y) > 100_000_000 {
            return Err("parameters exceed the checked 32-bit state/cost limits".into());
        }
        let mut bounds = vec![None; max_count + 1];
        bounds[0] = Some(Bounds::origin());
        for (i, v) in vectors.iter().enumerate() {
            extend_bounds(&mut bounds, *v, max_count.min(i + 1));
        }
        // INVARIANT: each count <= vectors.len() has at least one subset.
        let boxes: Vec<_> = bounds.into_iter().map(|b| b.unwrap()).collect();
        let cells: usize = boxes.iter().map(|b| b.cells()).sum();
        let bytes = cells.checked_mul(4).ok_or("table size overflow")?;
        if bytes > memory_mib * 1024 * 1024 {
            return Err(format!(
                "DP needs {:.1} MiB; limit is {memory_mib} MiB",
                bytes as f64 / 1048576.0
            ));
        }
        let mut tables: Vec<_> = boxes
            .into_iter()
            .map(|bounds| Table {
                costs: vec![INF; bounds.cells()],
                bounds,
            })
            .collect();
        tables[0].costs[0] = 0;
        let mut active = vec![None; max_count + 1];
        active[0] = Some(Bounds::origin());
        for (i, v) in vectors.iter().enumerate() {
            for count in (1..=max_count.min(i + 1)).rev() {
                // Descending counts ensure that this vector is used at most once.
                let (lower, upper) = tables.split_at_mut(count);
                // INVARIANT: count-1 <= i, so this prefix has a subset of that size.
                let source_bounds = active[count - 1].unwrap();
                extend_table(&lower[count - 1], &mut upper[0], source_bounds, *v);
            }
            extend_bounds(&mut active, *v, max_count.min(i + 1));
        }
        Ok(Self { tables, bytes })
    }

    pub fn join(&self, k: usize) -> Option<Winner> {
        let mut best = None;
        for count in 1..=k / 2 {
            let Some(left) = self.tables.get(count) else {
                continue;
            };
            let Some(right) = self.tables.get(k - count) else {
                continue;
            };
            let overlap = left.bounds.intersection(right.bounds);
            for y in overlap.ymin..=overlap.ymax {
                join_row(left, right, (overlap, y, count), &mut best);
            }
        }
        best
    }
}

fn extend_table(source: &Table, target: &mut Table, active: Bounds, v: Point) {
    let width = active.width();
    for y in active.ymin..=active.ymax {
        let p = Point { x: active.xmin, y };
        let start = source.index(p);
        let dest = target.index(p.plus(v));
        let mut increment = p.det(v);
        let source_row = &source.costs[start..start + width];
        let target_row = &mut target.costs[dest..dest + width];
        for (&cost, slot) in source_row.iter().zip(target_row) {
            if cost != INF {
                let candidate = i64::from(cost) + increment;
                // Valid finite states only contain earlier angular vectors.
                debug_assert!(candidate >= 0 && candidate < i64::from(INF));
                *slot = (*slot).min(candidate as i32);
            }
            increment += i64::from(v.y);
        }
    }
}

fn join_row(left: &Table, right: &Table, row: (Bounds, i32, usize), best: &mut Option<Winner>) {
    let (bounds, y, count) = row;
    if bounds.xmin > bounds.xmax {
        return;
    }
    let p = Point { x: bounds.xmin, y };
    let a = left.index(p);
    let b = right.index(p);
    let left_row = &left.costs[a..a + bounds.width()];
    let right_row = &right.costs[b..b + bounds.width()];
    for (offset, (&cost_a, &cost_b)) in left_row.iter().zip(right_row).enumerate() {
        if cost_a == INF || cost_b == INF {
            continue;
        }
        let area2 = i64::from(cost_a) + i64::from(cost_b);
        if best.is_none_or(|old| area2 < old.area2) {
            *best = Some(Winner {
                area2,
                count,
                sum: Point {
                    x: p.x + offset as i32,
                    y,
                },
            });
        }
    }
}

/// Hirschberg-style recovery: recompute the two angular subproblems, release
/// their tables before recursing, and account for cross pairs by det(g_left,g_right).
pub fn recover(
    vectors: &[Point],
    count: usize,
    sum: Point,
    memory_mib: usize,
) -> Result<Vec<Point>, String> {
    if count == 0 {
        return if sum == Point::default() {
            Ok(vec![])
        } else {
            Err("invalid empty witness".into())
        };
    }
    if count == vectors.len() {
        let total = vectors.iter().fold(Point::default(), |s, v| s.plus(*v));
        return if total == sum {
            Ok(vectors.to_vec())
        } else {
            Err("invalid full witness".into())
        };
    }
    if count == 1 {
        return if vectors.contains(&sum) {
            Ok(vec![sum])
        } else {
            Err("missing single vector".into())
        };
    }
    let middle = vectors.len() / 2;
    let left = Dp::build(&vectors[..middle], count, memory_mib)?;
    let remaining_mib = memory_mib.saturating_sub(left.bytes.div_ceil(1048576));
    let right = Dp::build(&vectors[middle..], count, remaining_mib)?;
    let (left_count, left_sum) = find_split(&left, &right, count, sum)?;
    drop(left);
    drop(right);
    let mut result = recover(&vectors[..middle], left_count, left_sum, memory_mib)?;
    result.extend(recover(
        &vectors[middle..],
        count - left_count,
        sum.minus(left_sum),
        memory_mib,
    )?);
    Ok(result)
}

fn find_split(left: &Dp, right: &Dp, count: usize, sum: Point) -> Result<(usize, Point), String> {
    let mut best = (i64::MAX, 0, Point::default());
    for used in 0..=count {
        let Some(a) = left.tables.get(used) else {
            continue;
        };
        let Some(b) = right.tables.get(count - used) else {
            continue;
        };
        for y in a.bounds.ymin..=a.bounds.ymax {
            for x in a.bounds.xmin..=a.bounds.xmax {
                let g = Point { x, y };
                let first = a.get(g);
                let second = b.get(sum.minus(g));
                if first == INF || second == INF {
                    continue;
                }
                let cost = i64::from(first) + i64::from(second) + g.det(sum);
                if cost < best.0 {
                    best = (cost, used, g);
                }
            }
        }
    }
    if best.0 == i64::MAX {
        return Err("no witness split".into());
    }
    Ok((best.1, best.2))
}
