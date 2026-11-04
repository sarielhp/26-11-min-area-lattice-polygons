use super::*;

#[test]
fn heuristic_selection_uses_final_count_and_preserves_domain_overrides() {
    let automatic = options(
        ["--heuristic", "--k", "95", "--classic"]
            .map(String::from)
            .into_iter(),
    )
    .unwrap()
    .unwrap();
    assert!(automatic.heuristic_pool && automatic.classic);
    assert_eq!(automatic.rectangles, vec![(8, 8)]);
    let fixed = options(
        ["--k", "95", "--box", "7", "7"]
            .map(String::from)
            .into_iter(),
    )
    .unwrap()
    .unwrap();
    assert!(!fixed.heuristic_pool);
    assert_eq!(fixed.rectangles, vec![(7, 7)]);
    let restored = options(
        ["--box", "7", "7", "--k", "95", "--heuristic"]
            .map(String::from)
            .into_iter(),
    )
    .unwrap()
    .unwrap();
    assert!(restored.heuristic_pool);
    assert_eq!(restored.rectangles, automatic.rectangles);
    let complete = options(
        ["--k", "32", "--budget", "1024"]
            .map(String::from)
            .into_iter(),
    )
    .unwrap()
    .unwrap();
    assert!(!complete.heuristic_pool);
    assert_eq!(complete.rectangles.len(), 11);
    assert_eq!(complete.rectangles.first(), Some(&(1, 1024)));
    assert_eq!(complete.rectangles.last(), Some(&(1024, 1)));
}

#[test]
fn heuristic_selection_rejects_invalid_counts_before_sizing() {
    for k in [0, 2, 1001, usize::MAX] {
        assert!(options(["--k".to_string(), k.to_string()].into_iter()).is_err());
    }
    let defaults = options(std::iter::empty()).unwrap().unwrap();
    assert!(defaults.heuristic_pool);
    assert_eq!(defaults.rectangles, vec![(12, 12)]);
}

#[test]
fn penalized_oracles_match_counted_dp() {
    for (a, b) in [(1, 1), (2, 1), (2, 2)] {
        for odd in [false, true] {
            let pool = geometry::upper_pool(a, b);
            let dp = fallback::Dp::build(&pool, pool.len(), 128).unwrap();
            let mut oracle = Oracle::new(&[(a, b)], odd, 128);
            for num in 0..=30 {
                let parameter = penalty::Penalty { num, den: 2 };
                let result = oracle.evaluate(parameter).unwrap();
                let mut pairs: Vec<_> = (3..=2 * pool.len())
                    .filter(|k| (k % 2 == 1) == odd)
                    .filter_map(|k| dp.join(k).map(|w| (2 * w.area2 - num * k as i64, k)))
                    .collect();
                if !odd {
                    pairs.extend([(0, 0), (-2 * num, 2)]);
                }
                let minimum = pairs.iter().map(|p| p.0).min().unwrap();
                let counts: Vec<_> = pairs
                    .iter()
                    .filter(|p| p.0 == minimum)
                    .map(|p| p.1)
                    .collect();
                assert_eq!(result.cost, minimum, "box={a},{b}; odd={odd}; p={num}");
                assert_eq!(result.low.winner.n, *counts.iter().min().unwrap());
                assert_eq!(result.high.winner.n, *counts.iter().max().unwrap());
            }
        }
    }
}

#[test]
fn small_search_and_penalized_recovery() {
    for k in 3..=25 {
        let mut oracle = Oracle::new(&[(4, 3)], k % 2 == 1, 128);
        let expected = fallback::Dp::build(&oracle.pools[0], k - 1, 128)
            .unwrap()
            .join(k)
            .unwrap();
        if let Some(found) = oracle.search(k).unwrap() {
            assert_eq!(found.winner.area2, expected.area2);
            assert_eq!(recover_penalty(&oracle, found).unwrap().len(), k);
        } else {
            assert_eq!(solve_fallback(&oracle, k, true).unwrap().0, expected.area2);
        }
    }
}

#[test]
fn multiple_rectangles_agree_with_original() {
    for k in 3..=11 {
        let oracle = Oracle::new(&[(1, 4), (2, 2), (4, 1)], k % 2 == 1, 128);
        let expected = solve_fallback(&oracle, k, false).unwrap().0;
        let mut oracle = oracle;
        if let Some(found) = oracle.search(k).unwrap() {
            assert_eq!(found.winner.area2, expected);
            assert_eq!(recover_penalty(&oracle, found).unwrap().len(), k);
        }
    }
}

#[test]
fn modular_oracles_match_counted_dp() {
    for (a, b) in [(1, 1), (2, 1), (2, 2)] {
        let pool = geometry::upper_pool(a, b);
        let dp = fallback::Dp::build(&pool, pool.len(), 128).unwrap();
        for k in 3..=2 * pool.len() {
            if dp.join(k).is_none() {
                continue;
            }
            for modulus in [2, 4, 8, 16] {
                let odd = k % 2 == 1;
                let mut oracle = Oracle::new(&[(a, b)], odd, 128);
                oracle.target = k;
                oracle.residues = if odd { modulus } else { modulus / 2 };
                for num in 0..=16 {
                    let result = oracle.evaluate(penalty::Penalty { num, den: 2 }).unwrap();
                    let mut pairs: Vec<_> = (3..=2 * pool.len())
                        .filter(|n| n % modulus == k % modulus)
                        .filter_map(|n| dp.join(n).map(|w| (2 * w.area2 - num * n as i64, n)))
                        .collect();
                    for n in [0, 2] {
                        if n % modulus == k % modulus {
                            pairs.push((-num * n as i64, n));
                        }
                    }
                    let minimum = pairs.iter().map(|p| p.0).min().unwrap();
                    let counts: Vec<_> = pairs
                        .iter()
                        .filter(|p| p.0 == minimum)
                        .map(|p| p.1)
                        .collect();
                    assert_eq!(
                        result.cost, minimum,
                        "box={a},{b}; k={k}; modulus={modulus}; p={num}"
                    );
                    assert_eq!(result.low.winner.n, *counts.iter().min().unwrap());
                    assert_eq!(result.high.winner.n, *counts.iter().max().unwrap());
                    for found in [result.low, result.high] {
                        if found.winner.n >= 3 {
                            assert_eq!(
                                recover_penalty(&oracle, found).unwrap().len(),
                                found.winner.n
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn skipped_count_requires_modular_refinement() {
    let mut oracle = Oracle::new(&[(6, 5)], true, 128);
    let found = oracle
        .search(59)
        .unwrap()
        .expect("no count-indexed fallback");
    assert_eq!(found.winner.area2, 6991);
    assert_eq!(oracle.residues, 4);
    assert_eq!(recover_penalty(&oracle, found).unwrap().len(), 59);
}

fn brute_polygon_areas(pool: &[Point]) -> Vec<Option<i64>> {
    let mut edges = pool.to_vec();
    edges.extend(pool.iter().copied().map(Point::negative));
    let mut best = vec![None; edges.len() + 1];
    for mask in 0_u64..1 << edges.len() {
        let mut current = Point::default();
        let mut area = 0;
        for (index, &edge) in edges.iter().enumerate() {
            if mask & (1 << index) == 0 {
                continue;
            }
            let next = current.plus(edge);
            area += current.det(next);
            current = next;
        }
        if current != Point::default() {
            continue;
        }
        let count = mask.count_ones() as usize;
        if best[count].is_none_or(|old| area < old) {
            best[count] = Some(area);
        }
    }
    best
}

#[test]
fn modular_search_matches_exhaustive_closed_polygons() {
    for (a, b) in [(1, 1), (2, 1), (2, 2)] {
        let expected = brute_polygon_areas(&geometry::upper_pool(a, b));
        for (k, area) in expected.iter().enumerate().skip(3) {
            let Some(area) = area else {
                continue;
            };
            let mut oracle = Oracle::new(&[(a, b)], k % 2 == 1, 128);
            let found = oracle.search(k).unwrap().expect("no fallback");
            assert_eq!(found.winner.area2, *area, "box={a},{b}; k={k}");
            assert_eq!(recover_penalty(&oracle, found).unwrap().len(), k);
        }
    }
}

#[test]
fn table_limits_fail_before_allocation() {
    let settings = penalty::Settings {
        penalty: penalty::Penalty { num: 1, den: 1 },
        residues: 2,
        cap: 2,
        memory_mib: 0,
    };
    assert!(penalty::Table::build(&geometry::upper_pool(1, 1), settings).is_err());
    let extreme = [Point { x: i32::MAX, y: 1 }; 2];
    assert!(penalty::Table::build(&extreme, settings).is_err());
}
