mod bounds;
mod fallback;
mod geometry;
mod penalty;
mod search;
#[cfg(test)]
mod tests;

use geometry::{Point, validate_polygon};
use search::Oracle;
use std::io::Write;
use std::time::Instant;

type Error = Box<dyn std::error::Error>;

struct Options {
    k: usize,
    rectangles: Vec<(i32, i32)>,
    memory_mib: usize,
    witness: Option<String>,
    classic: bool,
    heuristic_pool: bool,
}

/// Empirical rule validated through k=200; this is not a coverage theorem.
fn heuristic_side(k: usize) -> i32 {
    let mut side = 1;
    while 3 * side * side < 2 * k {
        side += 1;
    }
    side as i32
}

fn options(mut args: impl Iterator<Item = String>) -> Result<Option<Options>, Error> {
    let mut result = Options {
        k: 200,
        rectangles: Vec::new(),
        memory_mib: 4096,
        witness: None,
        classic: false,
        heuristic_pool: true,
    };
    while let Some(flag) = args.next() {
        if flag == "--classic" {
            result.classic = true;
            continue;
        }
        if flag == "--heuristic" {
            result.heuristic_pool = true;
            result.rectangles.clear();
            continue;
        }
        if flag == "--help" || flag == "-h" {
            println!(
                "lattice-parametric-dp [--k N] [--heuristic | --box A B | --budget B]\n\
                [--classic] [--memory-mib M] [--witness PATH]\n\
                Defaults: k=200, heuristic pool, memory=4096 MiB.\n\
                --heuristic uses box=sxs, s=ceil(sqrt(2*k/3)), for either algorithm.\n\
                This empirical pool rule does not certify global optimality.\n\
                Exact within the requested domain; increases the count modulus as needed.\n\
                --classic runs the original count-indexed DP for comparison.\n\
                Output is one JSON record. Witness format: count, then one x y per line."
            );
            return Ok(None);
        }
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--k" => result.k = value.parse()?,
            "--box" => {
                result.heuristic_pool = false;
                result.rectangles = vec![(
                    value.parse()?,
                    args.next().ok_or("missing height")?.parse()?,
                )];
            }
            "--budget" => {
                result.heuristic_pool = false;
                let budget: i32 = value.parse()?;
                if !(1..=100000).contains(&budget) {
                    return Err("budget out of range".into());
                }
                result.rectangles.clear();
                let mut a = 1;
                while a <= budget {
                    result.rectangles.push((a, budget / a));
                    a *= 2;
                }
            }
            "--memory-mib" => result.memory_mib = value.parse()?,
            "--witness" => result.witness = Some(value),
            _ => return Err(format!("unknown option {flag}").into()),
        }
    }
    if !(3..=1000).contains(&result.k)
        || !(1..=65536).contains(&result.memory_mib)
        || result
            .rectangles
            .iter()
            .any(|&(a, b)| a < 1 || b < 1 || i64::from(a) * i64::from(b) > 100000)
    {
        return Err("parameters out of supported range".into());
    }
    if result.heuristic_pool {
        let side = heuristic_side(result.k);
        result.rectangles = vec![(side, side)];
    }
    Ok(Some(result))
}

fn recover_penalty(oracle: &Oracle, found: search::Located) -> Result<Vec<Point>, Error> {
    let vectors = &oracle.pools[found.rectangle];
    let win = found.winner;
    let settings = penalty::Settings {
        penalty: found.penalty,
        residues: oracle.residues,
        cap: found.cap,
        memory_mib: oracle.memory_mib,
    };
    let mut edges = penalty::recover(vectors, win.sum, win.residue, win.maximum, settings)?;
    let second = if oracle.odd {
        penalty::recover(
            vectors,
            win.sum,
            (oracle.target % oracle.residues + oracle.residues - win.residue) % oracle.residues,
            win.maximum,
            settings,
        )?
    } else {
        edges.clone()
    };
    edges.extend(second.into_iter().map(Point::negative));
    if edges.len() != win.n {
        return Err("witness cardinality mismatch".into());
    }
    Ok(validate_polygon(&edges, win.area2)?)
}

fn solve_fallback(
    oracle: &Oracle,
    k: usize,
    witness: bool,
) -> Result<(i64, usize, Vec<Point>), Error> {
    let mut best: Option<(fallback::Winner, usize)> = None;
    let max_count = if k.is_multiple_of(2) { k / 2 } else { k - 1 };
    for (i, pool) in oracle.pools.iter().enumerate() {
        let dp = fallback::Dp::build(pool, max_count, oracle.memory_mib)?;
        let _array_bytes = dp.bytes;
        if let Some(win) = dp.join(k)
            && best.is_none_or(|old| win.area2 < old.0.area2)
        {
            best = Some((win, i));
        }
    }
    let (win, index) = best.ok_or("no polygon in the requested domain")?;
    if !witness {
        return Ok((win.area2, index, vec![]));
    }
    let pool = &oracle.pools[index];
    let mut edges = fallback::recover(pool, win.count, win.sum, oracle.memory_mib)?;
    let second = if k.is_multiple_of(2) {
        edges.clone()
    } else {
        fallback::recover(pool, k - win.count, win.sum, oracle.memory_mib)?
    };
    edges.extend(second.into_iter().map(Point::negative));
    Ok((win.area2, index, validate_polygon(&edges, win.area2)?))
}

fn run(options: Options) -> Result<(), Error> {
    let started = Instant::now();
    let mut oracle = Oracle::new(&options.rectangles, options.k % 2 == 1, options.memory_mib);
    let found = if options.classic {
        None
    } else {
        oracle.search(options.k)?
    };
    let search_seconds = started.elapsed().as_secs_f64();
    let (area2, index, vertices) = match found {
        Some(win) => (
            win.winner.area2,
            win.rectangle,
            if options.witness.is_some() {
                recover_penalty(&oracle, win)?
            } else {
                vec![]
            },
        ),
        None if options.classic => solve_fallback(&oracle, options.k, options.witness.is_some())?,
        None => return Err("modular search returned no answer".into()),
    };
    if let Some(path) = options.witness {
        let mut file = std::fs::File::create(path)?;
        writeln!(file, "{}", vertices.len())?;
        for point in vertices {
            writeln!(file, "{} {}", point.x, point.y)?;
        }
    }
    let peak = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|text| {
            text.lines()
                .find(|s| s.starts_with("VmHWM:"))
                .and_then(|s| s.split_whitespace().nth(1))
                .and_then(|s| s.parse::<u64>().ok())
        })
        .unwrap_or(0);
    println!(
        "{{\"k\":{},\"doubled_area\":{area2},\"classic\":{},\"heuristic_pool\":{},\"count_modulus\":{},\"penalty_queries\":{},\"rectangle_runs\":{},\"winning_box\":[{},{}],\"search_seconds\":{search_seconds:.6},\"total_seconds\":{:.6},\"peak_rss_kib\":{peak},\"peak_penalty_table_mib\":{:.3}}}",
        options.k,
        found.is_none(),
        options.heuristic_pool,
        if oracle.odd {
            oracle.residues
        } else {
            2 * oracle.residues
        },
        oracle.queries,
        oracle.rectangle_runs,
        options.rectangles[index].0,
        options.rectangles[index].1,
        started.elapsed().as_secs_f64(),
        oracle.peak_bytes as f64 / 1048576.0
    );
    Ok(())
}

fn main() {
    let outcome = options(std::env::args().skip(1)).and_then(|value| {
        if let Some(options) = value {
            run(options)
        } else {
            Ok(())
        }
    });
    if let Err(error) = outcome {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
