use super::*;

fn brute_areas(pool: &[Point]) -> Vec<Option<i64>> {
    let mut edges = pool.to_vec();
    edges.extend(pool.iter().copied().map(Point::negative));
    let mut best = vec![None; edges.len() + 1];
    for mask in 0_u64..1 << edges.len() {
        let mut point = Point::default();
        let mut area2 = 0;
        for (index, &edge) in edges.iter().enumerate() {
            if mask & (1 << index) != 0 {
                area2 += point.det(edge);
                point = point.plus(edge);
            }
        }
        if point != Point::default() {
            continue;
        }
        let count = mask.count_ones() as usize;
        if best[count].is_none_or(|old| area2 < old) {
            best[count] = Some(area2);
        }
    }
    best
}

#[test]
fn exact_count_dp_matches_exhaustive_closed_polygons() {
    let workers = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap();
    workers.install(|| {
        for (a, b) in [(1, 1), (2, 1), (2, 2)] {
            let pool = upper_pool(a, b);
            let expected = brute_areas(&pool);
            let dp = classic::Dp::build(&pool, pool.len(), 128).unwrap();
            for (k, area) in expected.iter().enumerate().skip(3) {
                assert_eq!(dp.join(k).map(|winner| winner.area2), *area);
            }
        }
    });
}

#[test]
fn thread_counts_preserve_values_and_reconstructed_coordinates() {
    let mut reference = Vec::new();
    for threads in [1, 2, 4] {
        let workers = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        let outcomes = workers.install(|| {
            [17, 32, 59]
                .map(|k| {
                    let options = options(["--k".to_string(), k.to_string()].into_iter())
                        .unwrap()
                        .unwrap();
                    let pools: Vec<_> = options
                        .rectangles
                        .iter()
                        .map(|&(a, b)| upper_pool(a, b))
                        .collect();
                    let (winner, index) = optimize(&pools, &options).unwrap();
                    let points = recover_polygon(&pools[index], winner, &options).unwrap();
                    assert_eq!(points.len(), k);
                    (winner.area2, points)
                })
                .to_vec()
        });
        if threads == 1 {
            reference = outcomes;
        } else {
            assert_eq!(outcomes, reference);
        }
    }
    assert_eq!(
        reference.iter().map(|r| r.0).collect::<Vec<_>>(),
        vec![151, 1046, 6991]
    );
}

#[test]
fn options_validate_threads_counts_and_explicit_domains() {
    for threads in [0, 129] {
        assert!(options(["--threads".to_string(), threads.to_string()].into_iter()).is_err());
    }
    assert!(options(["--k".to_string(), usize::MAX.to_string()].into_iter()).is_err());
    let fixed = options(
        ["--k", "95", "--box", "7", "7", "--threads", "2"]
            .map(String::from)
            .into_iter(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(fixed.rectangles, vec![(7, 7)]);
    assert!(!fixed.heuristic_pool);
    assert_eq!(fixed.threads, 2);
    let automatic = options(["--k", "95"].map(String::from).into_iter())
        .unwrap()
        .unwrap();
    assert!(automatic.heuristic_pool);
    assert_eq!(automatic.rectangles, vec![(8, 8)]);
}

#[test]
fn allocation_limit_is_still_enforced() {
    assert!(classic::Dp::build(&upper_pool(4, 4), 12, 0).is_err());
}

#[test]
fn memory_preflight_matches_allocations_and_covers_reconstruction() {
    for k in 3_usize..=16 {
        let options = options(["--k".to_string(), k.to_string()].into_iter())
            .unwrap()
            .unwrap();
        let (a, b) = options.rectangles[0];
        let pool = upper_pool(a, b);
        let count = if k.is_multiple_of(2) {
            k / 2
        } else {
            k.div_ceil(2)
        };
        assert_eq!(max_count(&options), count);
        let expected = classic::Dp::required_bytes(&pool, count).unwrap();
        let pools = vec![pool];
        let allowance = memory_allowance(&pools, &options).unwrap();
        let dp = classic::Dp::build(&pools[0], count, allowance).unwrap();
        assert_eq!(dp.bytes, expected);
        let win = dp.join(k).unwrap();
        drop(dp);
        let options = Options {
            memory_mib: allowance,
            ..options
        };
        assert_eq!(recover_polygon(&pools[0], win, &options).unwrap().len(), k);
    }
}

#[test]
fn certified_preflight_exposes_the_old_one_gib_limit_without_large_allocations() {
    let options = options(
        ["--k", "31", "--budget", "15472", "--estimate-memory"]
            .map(String::from)
            .into_iter(),
    )
    .unwrap()
    .unwrap();
    assert!(options.estimate_memory);
    let pools: Vec<_> = options
        .rectangles
        .iter()
        .map(|&(a, b)| upper_pool(a, b))
        .collect();
    assert_eq!(memory_allowance(&pools, &options).unwrap(), 1143);
}

#[test]
fn budget_metadata_tracks_the_selected_domain() {
    let complete = options(["--budget", "48"].map(String::from).into_iter())
        .unwrap()
        .unwrap();
    assert!(!complete.heuristic_pool);
    assert_eq!(complete.domain_budget, Some(48));
    assert_eq!(
        complete.rectangles,
        vec![(1, 48), (2, 24), (4, 12), (8, 6), (16, 3), (32, 1)]
    );
    for selector in [vec!["--heuristic"], vec!["--box", "2", "3"]] {
        let args = ["--budget", "48"]
            .into_iter()
            .chain(selector)
            .map(String::from);
        assert_eq!(options(args).unwrap().unwrap().domain_budget, None);
    }
}

#[test]
fn balanced_rectangles_are_searched_first_without_losing_any_domain() {
    let rectangles = [(1, 48), (2, 24), (4, 12), (8, 6), (16, 3), (32, 1)];
    assert_eq!(rectangle_order(&rectangles), vec![3, 2, 4, 1, 5, 0]);
    assert_eq!(rectangle_order(&[(4, 8), (8, 4)]), vec![0, 1]);
}

#[test]
fn early_pruning_preserves_every_affordable_state() {
    for threads in [1, 4] {
        let workers = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        workers.install(|| {
            for (a, b, count) in [(2, 2, 6), (1, 40, 8), (40, 1, 8), (7, 7, 16)] {
                let pool = upper_pool(a, b);
                let full = classic::Dp::build(&pool, count, 128).unwrap();
                for limit in [0, 1, 7, 65, 151] {
                    let pruned =
                        classic::Dp::build_with_limit(&pool, count, 128, Some(limit)).unwrap();
                    assert_affordable_states(&full, &pruned, limit);
                }
            }
        });
    }
}

fn assert_affordable_states(full: &classic::Dp, pruned: &classic::Dp, limit: i64) {
    assert_eq!(full.tables.len(), pruned.tables.len());
    for (original, capped) in full.tables.iter().zip(&pruned.tables) {
        assert_eq!(original.costs.len(), capped.costs.len());
        for (&cost, &actual) in original.costs.iter().zip(&capped.costs) {
            let expected = if i64::from(cost) <= limit {
                cost
            } else {
                classic::INF
            };
            assert_eq!(actual, expected, "limit={limit}");
        }
    }
}

#[test]
fn pruned_complete_search_matches_unpruned_optima_and_recovers_witnesses() {
    let mut options = options(["--budget", "48"].map(String::from).into_iter())
        .unwrap()
        .unwrap();
    let pools: Vec<_> = options
        .rectangles
        .iter()
        .map(|&(a, b)| upper_pool(a, b))
        .collect();
    for k in 3_usize..=12 {
        options.k = k;
        let count = if k.is_multiple_of(2) { k / 2 } else { k - 1 };
        let expected = pools
            .iter()
            .filter_map(|pool| {
                classic::Dp::build(pool, count, 128)
                    .unwrap()
                    .join(k)
                    .map(|win| win.area2)
            })
            .min()
            .unwrap();
        let (winner, index) = optimize(&pools, &options).unwrap();
        assert_eq!(winner.area2, expected);
        assert_eq!(
            recover_polygon(&pools[index], winner, &options)
                .unwrap()
                .len(),
            k
        );
    }
}

#[test]
fn balanced_odd_split_matches_all_splits_and_recovers_witnesses() {
    for k in (3_usize..=61).step_by(2) {
        let mut areas = Vec::new();
        for extra in [&[][..], &["--all-splits"][..]] {
            let mut args = vec![
                "--k".to_string(),
                k.to_string(),
                "--threads".to_string(),
                "1".to_string(),
            ];
            args.extend(extra.iter().map(|s| s.to_string()));
            let options = options(args.into_iter()).unwrap().unwrap();
            assert_eq!(balanced_split(&options), extra.is_empty());
            let pools: Vec<_> = options
                .rectangles
                .iter()
                .map(|&(a, b)| upper_pool(a, b))
                .collect();
            let (win, index) = optimize(&pools, &options).unwrap();
            assert_eq!(
                recover_polygon(&pools[index], win, &options).unwrap().len(),
                k
            );
            areas.push(win.area2);
        }
        assert_eq!(areas[0], areas[1], "k={k}");
    }
}

#[test]
fn explicit_domains_keep_every_odd_split() {
    let fixed = options(
        ["--k", "31", "--box", "5", "5"]
            .map(String::from)
            .into_iter(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(max_count(&fixed), 30);
    assert!(!balanced_split(&fixed));
}
