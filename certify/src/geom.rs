use crate::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Point {
    pub x: i64,
    pub y: i64,
}

impl Point {
    pub fn cross(self, other: Self) -> i128 {
        i128::from(self.x) * i128::from(other.y) - i128::from(self.y) * i128::from(other.x)
    }

    pub fn minus(self, other: Self) -> Self {
        Self {
            x: self.x - other.x,
            y: self.y - other.y,
        }
    }
}

pub fn gcd(mut a: i64, mut b: i64) -> i64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.abs()
}

/// Doubled-area lower bound for a k-gon of lattice width `width` (same rule as
/// `lattice-width-3/8-v1` in the Pareto backend).
pub fn width_lower_bound(k: usize, width: i64) -> i64 {
    let square = width * width;
    let bound = if k.is_multiple_of(2) {
        square
    } else {
        (3 * square + 3) / 4
    };
    bound + i64::from(bound % 2 != (k % 2) as i64)
}

pub fn validate(points: &[Point], k: usize, area2: i64) -> Result<(), Error> {
    if points.len() != k {
        return Err("incorrect witness vertex count".into());
    }
    let mut area = 0_i128;
    for (index, &a) in points.iter().enumerate() {
        let following = (index + 1) % k;
        let b = points[following];
        let edge = b.minus(a);
        if gcd(edge.x, edge.y) != 1 {
            return Err("witness contains a nonprimitive edge".into());
        }
        for (j, &p) in points.iter().enumerate() {
            if j != index && j != following && edge.cross(p.minus(a)) <= 0 {
                return Err("witness is not strictly convex and counterclockwise".into());
            }
        }
        area += a.cross(b);
    }
    if area != i128::from(area2) {
        return Err("witness area mismatch".into());
    }
    Ok(())
}
