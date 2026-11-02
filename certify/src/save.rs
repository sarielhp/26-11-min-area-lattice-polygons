//! Certified polygon files, written atomically as soon as each size is final.
//! Header lines: k, A063984(k), A070911(k), method, status, area, metadata.
use crate::Error;
use crate::geom::Point;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

pub struct Record<'a> {
    pub k: usize,
    pub area2: i64,
    pub points: &'a [Point],
    /// Rows searched for this k: L_k(height+1) >= area2 certifies it.
    pub height: usize,
    pub next_width_lower_bound: i64,
    pub threads: usize,
    pub run_target: usize,
    pub elapsed: f64,
}

pub fn path(dir: &Path, k: usize) -> PathBuf {
    dir.join(format!("k{k:06}.txt"))
}

fn saved_area(path: &Path, k: usize) -> Result<Option<i64>, Error> {
    if !path.exists() {
        return Ok(None);
    }
    let prefix = format!("# A070911({k}) = ");
    let text = std::fs::read_to_string(path)?;
    let line = text
        .lines()
        .find_map(|l| l.strip_prefix(&prefix))
        .ok_or("existing polygon file lacks an A070911 line")?;
    Ok(Some(line.trim().parse()?))
}

fn render(record: &Record) -> Result<String, Error> {
    let (k, area2) = (record.k, record.area2);
    let area = if area2 % 2 == 0 {
        format!("{}", area2 / 2)
    } else {
        format!("{}.5", area2 / 2)
    };
    let mut text = String::new();
    writeln!(text, "# k = {k}")?;
    writeln!(text, "# A063984({k}) = {}", (area2 - k as i64 + 2) / 2)?;
    writeln!(text, "# A070911({k}) = {area2}")?;
    writeln!(text, "# method = certified; algorithm = lattice-width-count-major")?;
    writeln!(text, "# status = globally optimal; complete-domain DP search")?;
    writeln!(text, "# area = {area}")?;
    writeln!(
        text,
        "# height_searched = {}; next_width_lower_bound = {}",
        record.height, record.next_width_lower_bound
    )?;
    writeln!(text, "# threads = {}; wall_seconds = {:.6}", record.threads, record.elapsed)?;
    writeln!(
        text,
        "# time_kind = cumulative run elapsed; run_target = {}",
        record.run_target
    )?;
    writeln!(
        text,
        "# metadata: {{\"k\":{k},\"doubled_area\":{area2},\"algorithm\":\"lattice-width-count-major\",\"width_bound_rule\":\"lattice-width-3/8-v1\",\"height_searched\":{},\"next_width_lower_bound\":{},\"threads\":{},\"run_target\":{},\"run_elapsed_seconds\":{:.6}}}",
        record.height, record.next_width_lower_bound, record.threads, record.run_target, record.elapsed
    )?;
    writeln!(text, "{k}")?;
    for p in record.points {
        writeln!(text, "{} {}", p.x, p.y)?;
    }
    Ok(text)
}

/// Writes the file unless it already holds the same area; a different area is an error.
pub fn write(dir: &Path, record: &Record) -> Result<bool, Error> {
    let target = path(dir, record.k);
    if let Some(old) = saved_area(&target, record.k)? {
        if old != record.area2 {
            return Err(format!(
                "k={}: computed doubled area {} differs from saved {}",
                record.k, record.area2, old
            )
            .into());
        }
        return Ok(false);
    }
    std::fs::create_dir_all(dir)?;
    let temporary = dir.join(format!(".k{:06}.tmp-{}", record.k, std::process::id()));
    std::fs::write(&temporary, render(record)?)?;
    std::fs::rename(&temporary, &target)?;
    Ok(true)
}
