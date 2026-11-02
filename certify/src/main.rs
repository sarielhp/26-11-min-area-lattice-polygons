mod checkpoint;
mod frontier;
mod geom;
mod layer;
mod save;
mod solve;
mod store;
#[cfg(test)]
mod tests;
mod turns;

use std::path::{Path, PathBuf};

type Error = Box<dyn std::error::Error + Send + Sync>;

const HELP: &str = "lattice-layered --k N [--threads T] [--upper PATH] [--lower PATH]
                [--spill DIR [--resume] | --no-witness] [--save DIR [--from K0]] [--verbose]
Count-major lattice-width DP: builds chain-vertex-count layers m=2,3,...,N,
keeping only two layers in RAM. Computes and certifies all minimum doubled
areas 3..N and (unless --no-witness) reconstructs and validates every polygon.
--upper/--lower: file of `k value` lines, or a directory of k*.txt polygon
files with an `A070911(k) = value` line. Upper values must be feasible doubled
areas (e.g. validated heuristic polygons); lower values must be certified.
The final searched height is fixed up front from the upper bounds.
--save DIR: write each certified polygon of size >= K0 (default 3) to DIR/kNNNNNN.txt
as soon as it is final (sizes certify in increasing k); an existing file with
a different area is an error, one with the same area is kept.
--resume: continue an interrupted run from the checkpoint in --spill DIR
(written after every layer; same --k, --upper, and --lower required). The
spill directory is kept after a failure and removed after success.
--stop-after M (testing): fail right after checkpointing layer M.";

struct Options {
    k: usize,
    threads: usize,
    upper: Option<PathBuf>,
    lower: Option<PathBuf>,
    spill: Option<PathBuf>,
    witness: bool,
    save: Option<PathBuf>,
    from: usize,
    resume: bool,
    stop_after: Option<usize>,
    verbose: bool,
}

fn options() -> Result<Option<Options>, Error> {
    let mut args = std::env::args().skip(1);
    let mut options = Options {
        k: 44,
        threads: std::thread::available_parallelism().map_or(1, usize::from),
        upper: None,
        lower: None,
        spill: None,
        witness: true,
        save: None,
        from: 3,
        resume: false,
        stop_after: None,
        verbose: false,
    };
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--help" | "-h" => {
                println!("{HELP}");
                return Ok(None);
            }
            "--no-witness" => options.witness = false,
            "--verbose" => options.verbose = true,
            "--resume" => options.resume = true,
            _ => {
                let value = args.next().ok_or_else(|| format!("missing value for {flag}"))?;
                match flag.as_str() {
                    "--k" => options.k = value.parse()?,
                    "--threads" => options.threads = value.parse()?,
                    "--upper" => options.upper = Some(value.into()),
                    "--lower" => options.lower = Some(value.into()),
                    "--spill" => options.spill = Some(value.into()),
                    "--save" => options.save = Some(value.into()),
                    "--from" => options.from = value.parse()?,
                    "--stop-after" => options.stop_after = Some(value.parse()?),
                    _ => return Err(format!("unknown option {flag}").into()),
                }
            }
        }
    }
    if !(3..=1000).contains(&options.k) || options.threads == 0 {
        return Err("supported: k=3..1000, threads>=1".into());
    }
    if options.resume && (options.spill.is_none() || !options.witness) {
        return Err("--resume needs an explicit --spill DIR (and witnesses)".into());
    }
    if options.save.is_some() && !options.witness {
        return Err("--save needs witnesses; drop --no-witness".into());
    }
    Ok(Some(options))
}

/// Reads `k value` lines, or the `A070911(k) = value` line of each file in a directory.
fn read_values(path: &Path, k_max: usize) -> Result<Vec<i64>, Error> {
    let mut values = vec![0_i64; k_max + 3];
    let mut put = |k: usize, v: i64| {
        if k < values.len() {
            values[k] = v;
        }
    };
    if path.is_dir() {
        for item in std::fs::read_dir(path)? {
            let text = std::fs::read_to_string(item?.path())?;
            for line in text.lines() {
                let Some(rest) = line.strip_prefix("# A070911(") else {
                    continue;
                };
                let (k, v) = rest.split_once(") = ").ok_or("malformed A070911 line")?;
                put(k.parse()?, v.trim().parse()?);
            }
        }
    } else {
        for line in std::fs::read_to_string(path)?.lines() {
            let mut fields = line.split_whitespace();
            if let (Some(k), Some(v)) = (fields.next(), fields.next())
                && let (Ok(k), Ok(v)) = (k.parse(), v.parse())
            {
                put(k, v);
            }
        }
    }
    Ok(values)
}

fn default_spill() -> PathBuf {
    let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
    PathBuf::from(format!(
        "/tmp/{user}/grid_smallest_area/layered-{}",
        std::process::id()
    ))
}

fn peak_rss_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("VmHWM:"))
                .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
        })
        .unwrap_or(0)
}

fn run(options: Options) -> Result<(), Error> {
    let k = options.k;
    // (i, i^2), 0 <= i < k, is a convex k-gon of doubled area k(k-1)(k-2)/3.
    let mut upper: Vec<i64> = (0..=k)
        .map(|n| {
            let n = n as i64;
            if n < 3 { 0 } else { n * (n - 1) * (n - 2) / 3 }
        })
        .collect();
    if let Some(path) = &options.upper {
        for (n, v) in read_values(path, k)?.into_iter().enumerate().take(k + 1) {
            if n >= 3 && v > 0 {
                upper[n] = upper[n].min(v);
            }
        }
    }
    let lower = match &options.lower {
        Some(path) => read_values(path, k)?,
        None => vec![0; k + 3],
    };
    let spill = options
        .witness
        .then(|| options.spill.clone().unwrap_or_else(default_spill));
    let config = solve::Config {
        k,
        threads: options.threads,
        upper,
        lower,
        spill,
        save: options.save.clone(),
        from: options.from,
        resume: options.resume,
        stop_after: options.stop_after,
        verbose: options.verbose,
    };
    let report = solve::run(&config)?;
    let areas: Vec<String> = (3..=k)
        .map(|n| report.solutions[n].map_or("null".into(), |s| s.area2.to_string()))
        .collect();
    let layers: Vec<String> = report
        .layers
        .iter()
        .map(|l| format!("[{},{},{},{},{:.3}]", l.m, l.entries, l.cap, l.h_limit, l.seconds))
        .collect();
    println!(
        "{{\"k\":{k},\"doubled_area\":{},\"algorithm\":\"lattice-width-count-major\",\"width_bound_rule\":\"lattice-width-3/8-v1\",\"threads\":{},\"h_max\":{},\"search_seconds\":{:.6},\"join_seconds\":{:.6},\"witnesses_validated\":{},\"generated_entries\":{},\"dominated_arrivals\":{},\"removed_entries\":{},\"total_entries\":{},\"peak_layer_bytes\":{},\"summary_bytes\":{},\"spill_bytes\":{},\"turn_table_entries\":{},\"peak_rss_kib\":{},\"areas2_from_3\":[{}],\"layers_m_entries_cap_hlimit_seconds\":[{}]}}",
        report.solutions[k].map_or(-1, |s| s.area2),
        options.threads,
        report.h_max,
        report.search_seconds,
        report.join_seconds,
        report.polygons.iter().filter(|p| p.is_some()).count(),
        report.stats.generated,
        report.stats.dominated,
        report.stats.removed,
        report.total_entries,
        report.peak_layer_bytes,
        report.summary_bytes,
        report.spill_bytes,
        report.turn_entries,
        peak_rss_kib(),
        areas.join(","),
        layers.join(",")
    );
    Ok(())
}

fn main() {
    let result = options().and_then(|o| o.map_or(Ok(()), run));
    if let Err(error) = result {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
