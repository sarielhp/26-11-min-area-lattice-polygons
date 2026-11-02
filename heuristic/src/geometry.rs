#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub fn det(self, other: Self) -> i64 {
        i64::from(self.x) * i64::from(other.y) - i64::from(self.y) * i64::from(other.x)
    }

    pub fn plus(self, other: Self) -> Self {
        Self {
            x: self.x + other.x,
            y: self.y + other.y,
        }
    }

    pub fn minus(self, other: Self) -> Self {
        Self {
            x: self.x - other.x,
            y: self.y - other.y,
        }
    }

    pub fn negative(self) -> Self {
        Self {
            x: -self.x,
            y: -self.y,
        }
    }
}

pub fn gcd(mut x: i32, mut y: i32) -> i32 {
    while y != 0 {
        (x, y) = (y, x % y);
    }
    x.abs()
}

pub fn upper_pool(width: i32, height: i32) -> Vec<Point> {
    let mut vectors = vec![Point { x: 1, y: 0 }];
    for y in 1..=height {
        for x in -width..=width {
            if gcd(x, y) == 1 {
                vectors.push(Point { x, y });
            }
        }
    }
    vectors.sort_unstable_by(|a, b| 0.cmp(&a.det(*b)));
    vectors
}

/// Validate a witness independently of the DP recurrence: every nonincident
/// vertex must be strictly left of every directed edge, and shoelace must agree.
pub fn validate_polygon(edges: &[Point], area2: i64) -> Result<Vec<Point>, String> {
    if edges.len() < 3 {
        return Err("fewer than three edges".into());
    }
    let mut point = Point::default();
    let mut vertices = Vec::with_capacity(edges.len());
    for edge in edges {
        vertices.push(point);
        point = point.plus(*edge);
    }
    if point != Point::default() {
        return Err("witness does not close".into());
    }
    for (i, edge) in edges.iter().enumerate() {
        for (j, vertex) in vertices.iter().enumerate() {
            if j == i || j == (i + 1) % edges.len() {
                continue;
            }
            if edge.det(vertex.minus(vertices[i])) <= 0 {
                return Err("witness is not strictly convex".into());
            }
        }
    }
    let shoelace: i64 = (0..vertices.len())
        .map(|i| vertices[i].det(vertices[(i + 1) % vertices.len()]))
        .sum();
    if shoelace != area2 {
        return Err(format!("shoelace {shoelace} differs from DP {area2}"));
    }
    Ok(vertices)
}
