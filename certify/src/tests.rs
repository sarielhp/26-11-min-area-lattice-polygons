use crate::frontier::{Packed, Stats, insert};
use crate::geom::Point;
use crate::solve::{Config, run};

// A070911(3..=40) from the OEIS b-file; used only to check results.
const REFERENCE: [i64; 38] = [
    1, 2, 5, 6, 13, 14, 21, 28, 43, 48, 65, 80, 103, 118, 151, 174, 213, 242, 289, 328, 387, 420,
    497, 548, 625, 690, 783, 860, 967, 1046, 1177, 1264, 1409, 1498, 1671, 1780, 1955, 2078,
];

fn constructive(k: usize) -> Vec<i64> {
    (0..=k as i64)
        .map(|n| if n < 3 { 0 } else { n * (n - 1) * (n - 2) / 3 })
        .collect()
}

#[test]
fn frontier_matches_bruteforce_pareto_set() {
    let mut seed = 12345_u64;
    for _ in 0..2000 {
        let mut entries = Vec::new();
        let mut all = Vec::new();
        let mut stats = Stats::default();
        for _ in 0..12 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let x = ((seed >> 33) % 13) as i64 - 6;
            let y = ((seed >> 45) % 5) as i64 + 1;
            let area = ((seed >> 20) % 30) as i64;
            let e = Packed::new(Point { x, y }, area).unwrap();
            all.push(e);
            insert(&mut entries, Point { x, y }, area as u32, &mut stats).unwrap();
        }
        // Brute force: keep e unless some other entry has area <= and slope >=,
        // with strict improvement in one coordinate (ties: keep one copy).
        let slope_ge = |a: Packed, b: Packed| {
            i64::from(a.x) * i64::from(b.y) >= i64::from(b.x) * i64::from(a.y)
        };
        for e in &all {
            let dominated = all.iter().any(|o| o.area <= e.area && slope_ge(*o, *e));
            assert!(dominated);
            let covered = entries.iter().any(|o| o.area <= e.area && slope_ge(*o, *e));
            assert!(covered, "{e:?} not covered by {entries:?}");
        }
        for pair in entries.windows(2) {
            assert!(pair[0].area < pair[1].area);
            assert!(!slope_ge(pair[0], pair[1]));
        }
    }
}

fn check(k: usize, upper: Vec<i64>, lower: Vec<i64>, threads: usize) {
    let dir = std::env::temp_dir().join(format!("layered-test-{}-{k}-{threads}", std::process::id()));
    let report = run(&Config {
        k,
        threads,
        upper,
        lower,
        spill: Some(dir),
        save: None,
        from: 3,
        resume: false,
        stop_after: None,
        verbose: false,
    })
    .unwrap();
    for n in 3..=k {
        assert_eq!(report.solutions[n].unwrap().area2, REFERENCE[n - 3], "k={n}");
        assert!(report.polygons[n].is_some());
    }
}

#[test]
fn constructive_bounds_reproduce_reference() {
    check(26, constructive(26), vec![0; 29], 1);
    check(26, constructive(26), vec![0; 29], 4);
}

#[test]
fn tight_upper_and_lower_bounds_reproduce_reference() {
    let mut exact = vec![0; 3];
    exact.extend(REFERENCE);
    let lower = exact.clone();
    exact.truncate(41);
    check(40, exact, lower, 3);
}

#[test]
fn invalid_upper_bound_is_reported() {
    let mut upper = constructive(12);
    upper[11] = REFERENCE[11 - 3] - 1;
    let result = run(&Config {
        k: 12,
        threads: 2,
        upper,
        lower: vec![0; 15],
        spill: None,
        save: None,
        from: 3,
        resume: false,
        stop_after: None,
        verbose: false,
    });
    assert!(result.is_err());
}

#[test]
fn save_writes_certified_files_and_rejects_conflicts() {
    let base = std::env::temp_dir().join(format!("layered-save-{}", std::process::id()));
    let save = base.join("out");
    let config = |save: &std::path::Path| Config {
        k: 20,
        threads: 2,
        upper: constructive(20),
        lower: vec![0; 23],
        spill: Some(base.join("spill")),
        save: Some(save.to_path_buf()),
        from: 15,
        resume: false,
        stop_after: None,
        verbose: false,
    };
    run(&config(&save)).unwrap();
    for k in 15..=20 {
        let text = std::fs::read_to_string(crate::save::path(&save, k)).unwrap();
        assert!(text.contains(&format!("# A070911({k}) = {}", REFERENCE[k - 3])));
        assert_eq!(text.lines().filter(|l| !l.starts_with('#')).count(), k + 1);
    }
    assert!(!crate::save::path(&save, 14).exists());
    // Same areas: kept. A conflicting saved area: error.
    run(&config(&save)).unwrap();
    let path = crate::save::path(&save, 17);
    let text = std::fs::read_to_string(&path).unwrap();
    let saved = format!("# A070911(17) = {}", REFERENCE[17 - 3]);
    assert!(text.contains(&saved));
    std::fs::write(&path, text.replace(&saved, "# A070911(17) = 1")).unwrap();
    assert!(run(&config(&save)).is_err());
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn resume_after_interruption_matches_uninterrupted_run() {
    let base = std::env::temp_dir().join(format!("layered-resume-{}", std::process::id()));
    let config = |resume: bool, stop_after: Option<usize>| Config {
        k: 26,
        threads: 3,
        upper: constructive(26),
        lower: vec![0; 29],
        spill: Some(base.join("spill")),
        save: Some(base.join("out")),
        from: 3,
        resume,
        stop_after,
        verbose: false,
    };
    let fresh = run(&Config { spill: Some(base.join("fresh")), save: None, ..config(false, None) }).unwrap();
    assert!(run(&config(false, Some(9))).is_err());
    // Sizes <= 9 are final and saved before the interruption.
    assert!(crate::save::path(&base.join("out"), 9).exists());
    assert!(!crate::save::path(&base.join("out"), 10).exists());
    assert!(run(&config(true, Some(13))).is_err());
    let resumed = run(&config(true, None)).unwrap();
    for k in 3..=26 {
        assert_eq!(resumed.solutions[k].unwrap().area2, fresh.solutions[k].unwrap().area2);
        assert_eq!(resumed.polygons[k], fresh.polygons[k], "k={k}");
        assert!(crate::save::path(&base.join("out"), k).exists());
    }
    assert_eq!(resumed.stats.generated, fresh.stats.generated);
    assert_eq!(resumed.total_entries, fresh.total_entries);
    // Success removes the spill, so a further resume has nothing to continue.
    assert!(run(&config(true, None)).is_err());
    std::fs::remove_dir_all(&base).unwrap();
}
