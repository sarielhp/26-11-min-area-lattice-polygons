//! Small text checkpoint written atomically after every completed layer.
//! Together with the spill directory's layer and summary files it is enough to
//! continue an interrupted run at the next layer with identical results.
use crate::Error;
use crate::frontier::Stats;
use crate::geom::Point;
use crate::solve::{Join, Solution};
use std::fmt::Write as _;
use std::path::Path;

pub const FILE: &str = "checkpoint.txt";
const MAGIC: &str = "lattice-layered-checkpoint v1";

/// Run parameters that determine every layer; a resume must match them exactly.
#[derive(Debug, PartialEq, Eq)]
pub struct Fingerprint {
    pub k: usize,
    pub h_max: usize,
    pub upper: Vec<i64>,
    pub lower: Vec<i64>,
}

pub struct State {
    pub fingerprint: Fingerprint,
    pub last_layer: usize,
    pub best: Vec<Option<Solution>>,
    pub stats: Stats,
    pub total_entries: u64,
    pub peak_layer_bytes: usize,
}

fn join_name(join: Join) -> &'static str {
    match join {
        Join::Diagonal => "diagonal",
        Join::OneFlat => "one-flat",
        Join::TwoFlat => "two-flat",
    }
}

fn join_from(name: &str) -> Result<Join, Error> {
    match name {
        "diagonal" => Ok(Join::Diagonal),
        "one-flat" => Ok(Join::OneFlat),
        "two-flat" => Ok(Join::TwoFlat),
        _ => Err(format!("checkpoint: unknown join {name}").into()),
    }
}

fn list(values: &[i64]) -> String {
    values.iter().map(i64::to_string).collect::<Vec<_>>().join(" ")
}

pub fn save(dir: &Path, state: &State) -> Result<(), Error> {
    let f = &state.fingerprint;
    let mut text = String::new();
    writeln!(text, "{MAGIC}")?;
    writeln!(text, "k {}", f.k)?;
    writeln!(text, "h_max {}", f.h_max)?;
    writeln!(text, "upper {}", list(&f.upper))?;
    writeln!(text, "lower {}", list(&f.lower))?;
    writeln!(text, "last_layer {}", state.last_layer)?;
    let s = state.stats;
    writeln!(text, "stats {} {} {}", s.generated, s.dominated, s.removed)?;
    writeln!(text, "total_entries {}", state.total_entries)?;
    writeln!(text, "peak_layer_bytes {}", state.peak_layer_bytes)?;
    for (k, solution) in state.best.iter().enumerate() {
        if let Some(b) = solution {
            writeln!(
                text,
                "best {k} {} {} {} {} {} {}",
                b.area2,
                b.top.x,
                b.top.y,
                b.right_count,
                b.left_count,
                join_name(b.join)
            )?;
        }
    }
    let temporary = dir.join(format!("{FILE}.tmp"));
    std::fs::write(&temporary, text)?;
    std::fs::rename(temporary, dir.join(FILE))?;
    Ok(())
}

fn numbers<T: std::str::FromStr>(fields: &[&str]) -> Result<Vec<T>, Error>
where
    T::Err: std::error::Error + Send + Sync + 'static,
{
    fields.iter().map(|v| Ok(v.parse::<T>()?)).collect()
}

fn best_line(fields: &[&str], best: &mut [Option<Solution>]) -> Result<(), Error> {
    let [k, area2, x, y, right, left, join] = fields else {
        return Err("checkpoint: malformed best line".into());
    };
    let k: usize = k.parse()?;
    let slot = best.get_mut(k).ok_or("checkpoint: best size out of range")?;
    *slot = Some(Solution {
        area2: area2.parse()?,
        top: Point {
            x: x.parse()?,
            y: y.parse()?,
        },
        right_count: right.parse()?,
        left_count: left.parse()?,
        join: join_from(join)?,
    });
    Ok(())
}

pub fn load(dir: &Path) -> Result<State, Error> {
    let text = std::fs::read_to_string(dir.join(FILE))
        .map_err(|e| format!("no checkpoint in {}: {e}", dir.display()))?;
    let mut lines = text.lines();
    if lines.next() != Some(MAGIC) {
        return Err("not a lattice-layered checkpoint".into());
    }
    let mut state = State {
        fingerprint: Fingerprint {
            k: 0,
            h_max: 0,
            upper: Vec::new(),
            lower: Vec::new(),
        },
        last_layer: 0,
        best: Vec::new(),
        stats: Stats::default(),
        total_entries: 0,
        peak_layer_bytes: 0,
    };
    for line in lines {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let Some((&key, rest)) = fields.split_first() else {
            continue;
        };
        let one = || rest.first().copied().ok_or("checkpoint: missing value");
        match key {
            "k" => {
                state.fingerprint.k = one()?.parse()?;
                state.best = vec![None; state.fingerprint.k + 1];
            }
            "h_max" => state.fingerprint.h_max = one()?.parse()?,
            "upper" => state.fingerprint.upper = numbers(rest)?,
            "lower" => state.fingerprint.lower = numbers(rest)?,
            "last_layer" => state.last_layer = one()?.parse()?,
            "stats" => {
                let [g, d, r] = numbers::<u64>(rest)?[..] else {
                    return Err("checkpoint: malformed stats".into());
                };
                state.stats = Stats {
                    generated: g,
                    dominated: d,
                    removed: r,
                };
            }
            "total_entries" => state.total_entries = one()?.parse()?,
            "peak_layer_bytes" => state.peak_layer_bytes = one()?.parse()?,
            "best" => best_line(rest, &mut state.best)?,
            _ => return Err(format!("checkpoint: unknown key {key}").into()),
        }
    }
    if state.last_layer < 2 {
        return Err("checkpoint: no completed layer".into());
    }
    Ok(state)
}
