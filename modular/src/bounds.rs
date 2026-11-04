use crate::geometry::Point;
use crate::penalty::Penalty;

fn area(pool: &[Point], selected: &[bool]) -> i64 {
    let mut sum = Point::default();
    let mut cost = 0;
    for (&vector, &take) in pool.iter().zip(selected) {
        if take {
            cost += sum.det(vector);
            sum = sum.plus(vector);
        }
    }
    cost
}

/// Shortest-vector feasible solutions, used only as objective upper bounds.
fn seed_area(pool: &[Point], count: usize, odd: bool) -> Option<i64> {
    let mut order: Vec<_> = (0..pool.len()).collect();
    order.sort_unstable_by_key(|&i| {
        let p = pool[i];
        (i64::from(p.x).pow(2) + i64::from(p.y).pow(2), p.x, p.y)
    });
    let mut left = vec![false; pool.len()];
    if !odd {
        if count > pool.len() {
            return None;
        }
        for &i in order.iter().take(count) {
            left[i] = true;
        }
        return Some(area(pool, &left));
    }
    if count < 3 || count > 2 * pool.len() - 3 {
        return None;
    }
    let axis = pool.iter().position(|v| *v == Point { x: 1, y: 0 })?;
    let diagonal = pool.iter().position(|v| *v == Point { x: -1, y: 1 })?;
    let vertical = pool.iter().position(|v| *v == Point { x: 0, y: 1 })?;
    let mut right = left.clone();
    left[axis] = true;
    left[diagonal] = true;
    right[vertical] = true;
    for &i in order
        .iter()
        .filter(|&&i| i != axis && i != diagonal && i != vertical)
        .take((count - 3) / 2)
    {
        left[i] = true;
        right[i] = true;
    }
    Some(area(pool, &left) + area(pool, &right))
}

/// Every globally minimizing chain must satisfy this necessary cubic bound.
/// All quantities are multiplied by ten to avoid floating-point rounding.
pub fn count_cap(
    pool: &[Point],
    penalty: Penalty,
    k: usize,
    residues: usize,
    odd: bool,
) -> Option<usize> {
    let target = if odd { k } else { k / 2 };
    let residue = target % residues;
    let smallest = if odd && residue < 3 {
        residue + residues
    } else {
        residue
    };
    let upper = [smallest, target]
        .into_iter()
        .filter_map(|count| {
            seed_area(pool, count, odd).map(|cost| {
                10 * (i128::from(penalty.den) * i128::from(cost)
                    - i128::from(penalty.num) * count as i128)
            })
        })
        .min()?;
    let lower: Vec<_> = (0..=pool.len())
        .map(|count| {
            let area = if count >= 2 {
                (count as i128).pow(3)
            } else {
                0
            };
            i128::from(penalty.den) * area - 10 * i128::from(penalty.num) * count as i128
        })
        .collect();
    if !odd {
        return (0..=pool.len())
            .filter(|&u| u % residues == residue && lower[u] <= upper)
            .max()
            .map(|u| u.max(1));
    }
    let mut minimum = vec![i128::MAX; residues];
    for (count, &value) in lower.iter().enumerate() {
        minimum[count % residues] = minimum[count % residues].min(value);
    }
    (0..=pool.len())
        .filter(|&u| {
            let other = minimum[(residue + residues - u % residues) % residues];
            other != i128::MAX && lower[u] + other <= upper
        })
        .max()
        .map(|u| u.max(1))
}
