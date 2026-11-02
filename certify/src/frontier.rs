use crate::Error;
use crate::geom::Point;

/// Inline 8-byte frontier entry: incoming edge and doubled chain area.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Packed {
    pub x: i16,
    pub y: u16,
    pub area: u32,
}

impl Packed {
    pub fn new(edge: Point, area: i64) -> Result<Self, Error> {
        Ok(Self {
            x: i16::try_from(edge.x).map_err(|_| "incoming x exceeds packed i16 range")?,
            y: u16::try_from(edge.y).map_err(|_| "incoming y exceeds packed u16 range")?,
            area: u32::try_from(area).map_err(|_| "area exceeds packed u32 range")?,
        })
    }

    #[inline]
    pub fn incoming(self) -> Point {
        Point {
            x: i64::from(self.x),
            y: i64::from(self.y),
        }
    }

    #[inline]
    pub fn area2(self) -> i64 {
        i64::from(self.area)
    }

    pub fn to_bytes(self) -> [u8; 8] {
        let mut out = [0; 8];
        out[0..2].copy_from_slice(&self.x.to_le_bytes());
        out[2..4].copy_from_slice(&self.y.to_le_bytes());
        out[4..8].copy_from_slice(&self.area.to_le_bytes());
        out
    }

    pub fn from_bytes(b: &[u8]) -> Self {
        Self {
            x: i16::from_le_bytes([b[0], b[1]]),
            y: u16::from_le_bytes([b[2], b[3]]),
            area: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
        }
    }
}

/// a.x/a.y < b.x/b.y (a is the stricter continuation constraint).
#[inline]
fn slope_lt(a: (i64, i64), b: (i64, i64)) -> bool {
    a.0 * b.1 < b.0 * a.1
}

#[inline]
fn slope(e: Packed) -> (i64, i64) {
    (i64::from(e.x), i64::from(e.y))
}

#[derive(Default, Clone, Copy)]
pub struct Stats {
    pub generated: u64,
    pub dominated: u64,
    pub removed: u64,
}

impl Stats {
    pub fn add(&mut self, other: Stats) {
        self.generated += other.generated;
        self.dominated += other.dominated;
        self.removed += other.removed;
    }
}

/// Frontier invariant: slope and area both strictly increase. The candidate is
/// converted to a packed entry only if it survives the dominance test.
#[inline]
pub fn insert(
    entries: &mut Vec<Packed>,
    edge: Point,
    area: u32,
    stats: &mut Stats,
) -> Result<(), Error> {
    stats.generated += 1;
    let candidate = (edge.x, edge.y);
    // O(1) rejection by the most permissive (and most expensive) entry.
    if let Some(&last) = entries.last()
        && last.area <= area
        && !slope_lt(slope(last), candidate)
    {
        stats.dominated += 1;
        return Ok(());
    }
    // The most permissive entry no more expensive than the candidate.
    let cheaper = entries.partition_point(|old| old.area <= area);
    if cheaper > 0 && !slope_lt(slope(entries[cheaper - 1]), candidate) {
        stats.dominated += 1;
        return Ok(());
    }
    let packed = Packed::new(edge, i64::from(area))?;
    let index = entries.partition_point(|&old| slope_lt(slope(old), candidate));
    let start = entries[..index].partition_point(|old| old.area < area);
    let equal = index < entries.len() && !slope_lt(candidate, slope(entries[index]));
    let end = index + usize::from(equal);
    stats.removed += (end - start) as u64;
    if start == end {
        entries.insert(start, packed);
    } else {
        entries[start] = packed;
        entries.drain(start + 1..end);
    }
    Ok(())
}
