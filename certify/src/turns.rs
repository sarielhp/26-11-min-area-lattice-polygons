//! Empty-ear successor lists (same construction as rust_min_k_gon_pareto/src/turns.rs).
#[derive(Clone, Copy, Debug)]
pub struct Turn {
    pub incoming_x: i64,
    pub outgoing_x: i64,
}

pub struct TurnTable {
    // rows[sum][incoming_y], sorted by (incoming_x, outgoing_x).
    rows: Vec<Vec<Vec<Turn>>>,
    pub entries: usize,
}

fn inverse(value: i64, modulus: i64) -> Option<i64> {
    let (mut old_r, mut remainder) = (modulus, value);
    let (mut old_t, mut coefficient) = (0, 1);
    while remainder != 0 {
        let quotient = old_r / remainder;
        (old_r, remainder) = (remainder, old_r - quotient * remainder);
        (old_t, coefficient) = (coefficient, old_t - quotient * coefficient);
    }
    (old_r == 1).then(|| old_t.rem_euclid(modulus))
}

impl TurnTable {
    pub fn new(max_sum: usize) -> Self {
        let mut rows = Vec::with_capacity(max_sum + 1);
        let mut entries = 0;
        for sum in 0..=max_sum as i64 {
            let mut row: Vec<Vec<Turn>> = (0..sum).map(|_| Vec::new()).collect();
            for determinant in (1..=sum).filter(|d| sum % d == 0) {
                for vertical in 1..sum {
                    let Some(horizontal) = inverse(sum / determinant, vertical) else {
                        continue;
                    };
                    let outgoing_x = (horizontal * (sum - vertical) - determinant) / vertical;
                    row[vertical as usize].push(Turn {
                        incoming_x: horizontal,
                        outgoing_x,
                    });
                    entries += 1;
                }
            }
            for turns in &mut row {
                turns.sort_unstable_by_key(|turn| (turn.incoming_x, turn.outgoing_x));
            }
            rows.push(row);
        }
        Self { rows, entries }
    }

    /// Successors g=(outgoing_x + shift, dy) of an incoming f with vertical step `fy`
    /// and `residue = f.x mod fy`, sorted by increasing outgoing_x; the caller adds
    /// `floor(f.x / fy) * dy`.
    #[inline]
    pub fn turns(&self, fy: i64, residue: i64, dy: i64) -> &[Turn] {
        let list = &self.rows[(fy + dy) as usize][fy as usize];
        let start = list.partition_point(|turn| turn.incoming_x < residue);
        let end = start + list[start..].partition_point(|turn| turn.incoming_x <= residue);
        &list[start..end]
    }
}
