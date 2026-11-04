mod classic;
mod geometry;
mod hybrid;
#[cfg(test)]
mod tests;

use geometry::{Point, upper_pool, validate_polygon};
use std::io::Write;
use std::time::Instant;

type Error = Box<dyn std::error::Error + Send + Sync>;

struct Options {
    k: usize,
    rectangles: Vec<(i32, i32)>,
    memory_mib: usize,
    witness: Option<String>,
    threads: usize,
    heuristic_pool: bool,
    domain_budget: Option<i32>,
    estimate_memory: bool,
    all_splits: bool,
    hybrid: Option<i32>,
    spill: Option<String>,
    out: Option<String>,
}

fn options(mut args: impl Iterator<Item = String>) -> Result<Option<Options>, Error> {
    let mut result = Options {
        k: 200,
        rectangles: Vec::new(),
        memory_mib: 4096,
        witness: None,
        // One worker is a safe fallback if the OS cannot report CPU availability.
        threads: std::thread::available_parallelism().map_or(1, |n| n.get().min(4)),
        heuristic_pool: true,
        domain_budget: None,
        estimate_memory: false,
        all_splits: false,
        hybrid: None,
        spill: None,
        out: None,
    };
    while let Some(flag) = args.next() {
        if flag == "--estimate-memory" {
            result.estimate_memory = true;
            continue;
        }
        if flag == "--all-splits" {
            result.all_splits = true;
            continue;
        }
        if flag == "--heuristic" {
            result.heuristic_pool = true;
            result.rectangles.clear();
            result.domain_budget = None;
            continue;
        }
        if flag == "--help" || flag == "-h" {
            println!(
                "lattice-classic-parallel [--k N] [--threads T]\n\
                [--heuristic | --box A B | --budget B] [--all-splits] [--memory-mib M] [--witness PATH]\n\
                Defaults: k=200, heuristic pool, up to 4 threads, memory=4096 MiB.\n\
                Heuristic box=sxs, s=ceil(sqrt(2*k/3)); global coverage is not proved.\n\
                Heuristic odd k joins only (k-1)/2 + (k+1)/2 edges; --all-splits joins every split.\n\
                --hybrid S --spill DIR --out DIR: count-major run for every k with box side S;\n\
                writes DIR/kNNNNNN.txt witnesses and one JSON line with all areas.\n\
                Classic exact-count DP; --threads 1 uses serial updates.\n\
                --estimate-memory: report DP allowance including reconstruction, without solving.\n\
                Output: one JSON record. Witness: count, then one x y per line."
            );
            return Ok(None);
        }
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--k" => result.k = value.parse()?,
            "--threads" => result.threads = value.parse()?,
            "--memory-mib" => result.memory_mib = value.parse()?,
            "--witness" => result.witness = Some(value),
            "--hybrid" => result.hybrid = Some(value.parse()?),
            "--spill" => result.spill = Some(value),
            "--out" => result.out = Some(value),
            "--box" => {
                result.heuristic_pool = false;
                result.domain_budget = None;
                result.rectangles = vec![(
                    value.parse()?,
                    args.next().ok_or("missing height")?.parse()?,
                )];
            }
            "--budget" => {
                let budget: i32 = value.parse()?;
                if !(1..=100000).contains(&budget) {
                    return Err("budget out of range".into());
                }
                result.heuristic_pool = false;
                result.domain_budget = Some(budget);
                result.rectangles.clear();
                let mut a = 1;
                while a <= budget {
                    result.rectangles.push((a, budget / a));
                    a *= 2;
                }
            }
            _ => return Err(format!("unknown option {flag}").into()),
        }
    }
    if !(3..=1000).contains(&result.k)
        || !(1..=128).contains(&result.threads)
        || !(1..=65536).contains(&result.memory_mib)
        || result
            .rectangles
            .iter()
            .any(|&(a, b)| a < 1 || b < 1 || i64::from(a) * i64::from(b) > 100000)
    {
        return Err("parameters out of supported range".into());
    }
    if result.heuristic_pool {
        let mut side = 1;
        while 3 * side * side < 2 * result.k {
            side += 1;
        }
        result.rectangles = vec![(side as i32, side as i32)];
    }
    Ok(Some(result))
}

/// Largest chain count kept in the tables. Even k uses a centrally symmetric
/// k/2 + k/2 join. Odd k with the heuristic pool joins only the balanced split
/// (k-1)/2 + (k+1)/2: every convex polygon has a lattice direction with that
/// split, but the box is not invariant under the map sending it to (1,0), so
/// this is part of the heuristic. Explicit domains keep every split.
fn max_count(options: &Options) -> usize {
    let k = options.k;
    if k.is_multiple_of(2) {
        k / 2
    } else if options.heuristic_pool && !options.all_splits {
        k.div_ceil(2)
    } else {
        k - 1
    }
}

fn balanced_split(options: &Options) -> bool {
    options.k.is_multiple_of(2) || (options.heuristic_pool && !options.all_splits)
}

fn rectangle_order(rectangles: &[(i32, i32)]) -> Vec<usize> {
    let mut order: Vec<_> = (0..rectangles.len()).collect();
    // Compare aspect ratios exactly; retain input order when ratios tie.
    order.sort_by(|&i, &j| {
        let (a, b) = rectangles[i];
        let (c, d) = rectangles[j];
        (i64::from(a.max(b)) * i64::from(c.min(d)))
            .cmp(&(i64::from(c.max(d)) * i64::from(a.min(b))))
    });
    order
}

fn optimize(pools: &[Vec<Point>], options: &Options) -> Result<(classic::Winner, usize), Error> {
    let mut best: Option<(classic::Winner, usize)> = None;
    let max_count = max_count(options);
    // Rectangles remain sequential, so the memory budget is shared rather than multiplied.
    for index in rectangle_order(&options.rectangles) {
        // Seek a strict improvement. Upper-half-plane vectors are ordered by
        // angle, so every added determinant is nonnegative. An odd polygon
        // joins two nonnegative chain costs; an even polygon doubles one.
        let limit = best.map(|(win, _)| {
            let area2 = win.area2 - 1;
            if options.k.is_multiple_of(2) {
                area2 / 2
            } else {
                area2
            }
        });
        let dp =
            classic::Dp::build_with_limit(&pools[index], max_count, options.memory_mib, limit)?;
        if let Some(win) = dp.join(options.k)
            && best.is_none_or(|old| win.area2 < old.0.area2)
        {
            best = Some((win, index));
        }
    }
    best.ok_or_else(|| "no polygon in the requested domain".into())
}

fn recover_polygon(
    pool: &[Point],
    win: classic::Winner,
    options: &Options,
) -> Result<Vec<Point>, Error> {
    let mut edges = classic::recover(pool, win.count, win.sum, options.memory_mib)?;
    let second = if options.k.is_multiple_of(2) {
        edges.clone()
    } else {
        classic::recover(pool, options.k - win.count, win.sum, options.memory_mib)?
    };
    edges.extend(second.into_iter().map(Point::negative));
    Ok(validate_polygon(&edges, win.area2)?)
}

fn memory_allowance(pools: &[Vec<Point>], options: &Options) -> Result<usize, Error> {
    let max_count = max_count(options);
    let mut bytes = 0;
    for pool in pools {
        let search = classic::Dp::required_bytes(pool, max_count)?;
        let middle = pool.len() / 2;
        let left = classic::Dp::required_bytes(&pool[..middle], max_count)?;
        let right = classic::Dp::required_bytes(&pool[middle..], max_count)?;
        // Every deeper recovery table uses a subset of one of these halves and
        // a count <= max_count, so its bounding boxes cannot be larger.
        // Recovery holds two tables, drops them, then recurses. One extra MiB
        // covers rounding when recovery subtracts the first table's allowance.
        let recovery = 2 * left.max(right) + 1048576;
        bytes = bytes.max(search).max(recovery);
    }
    Ok(bytes.div_ceil(1048576))
}

/// Box side of the heuristic pool for count k: s = ceil(sqrt(2k/3)).
fn heuristic_side(k: usize) -> i32 {
    let mut side = 1;
    while 3 * side * side < 2 * k as i32 {
        side += 1;
    }
    side
}

fn run_hybrid(options: &Options, side: i32, started: Instant) -> Result<(), Error> {
    let spill = options.spill.as_ref().ok_or("--hybrid needs --spill DIR")?;
    let out = options.out.as_ref().ok_or("--hybrid needs --out DIR")?;
    let ks: Vec<usize> = (3..=1000).filter(|&k| heuristic_side(k) == side).collect();
    if ks.is_empty() {
        return Err("no count uses that box side".into());
    }
    let pool = upper_pool(side, side);
    let report = hybrid::run(&pool, &ks, std::path::Path::new(spill))?;
    std::fs::create_dir_all(out)?;
    let mut areas = Vec::new();
    for r in &report.results {
        let mut file =
            std::fs::File::create(std::path::Path::new(out).join(format!("k{:06}.txt", r.k)))?;
        writeln!(file, "{}", r.vertices.len())?;
        for p in &r.vertices {
            writeln!(file, "{} {}", p.x, p.y)?;
        }
        areas.push(format!("[{},{}]", r.k, r.area2));
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
        "{{\"side\":{side},\"k_first\":{},\"k_last\":{},\"threads\":{},\"areas\":[{}],\"records\":{},\"stream_bytes\":{},\"peak_table_bytes\":{},\"seconds\":{:.3},\"peak_rss_kib\":{peak}}}",
        ks[0],
        ks[ks.len() - 1],
        options.threads,
        areas.join(","),
        report.records,
        report.stream_bytes,
        report.peak_table_bytes,
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

fn run(options: &Options, started: Instant) -> Result<(), Error> {
    let pools: Vec<_> = options
        .rectangles
        .iter()
        .map(|&(a, b)| upper_pool(a, b))
        .collect();
    if options.estimate_memory {
        let memory = memory_allowance(&pools, options)?;
        println!("{{\"k\":{},\"dp_memory_mib\":{memory}}}", options.k);
        return Ok(());
    }
    let (winner, index) = optimize(&pools, options)?;
    let search_seconds = started.elapsed().as_secs_f64();
    if let Some(path) = &options.witness {
        let points = recover_polygon(&pools[index], winner, options)?;
        let mut file = std::fs::File::create(path)?;
        writeln!(file, "{}", points.len())?;
        for point in points {
            writeln!(file, "{} {}", point.x, point.y)?;
        }
    }
    // RSS is optional telemetry on platforms without Linux /proc.
    let peak = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|text| {
            text.lines()
                .find(|s| s.starts_with("VmHWM:"))
                .and_then(|s| s.split_whitespace().nth(1))
                .and_then(|s| s.parse::<u64>().ok())
        })
        .unwrap_or(0);
    let domain_budget = options
        .domain_budget
        .map_or("null".to_string(), |b| b.to_string());
    println!(
        "{{\"k\":{},\"doubled_area\":{},\"classic\":true,\"threads\":{},\"heuristic_pool\":{},\"balanced_split\":{},\"domain_budget\":{domain_budget},\"rectangles_searched\":{},\"winning_box\":[{},{}],\"search_seconds\":{search_seconds:.6},\"total_seconds\":{:.6},\"peak_rss_kib\":{peak}}}",
        options.k,
        winner.area2,
        options.threads,
        options.heuristic_pool,
        balanced_split(options),
        options.rectangles.len(),
        options.rectangles[index].0,
        options.rectangles[index].1,
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

fn main() {
    let outcome = options(std::env::args().skip(1)).and_then(|options| {
        let Some(options) = options else {
            return Ok(());
        };
        let started = Instant::now();
        let workers = rayon::ThreadPoolBuilder::new()
            .num_threads(options.threads)
            .build()?;
        workers.install(|| match options.hybrid {
            Some(side) => run_hybrid(&options, side, started),
            None => run(&options, started),
        })
    });
    if let Err(error) = outcome {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
