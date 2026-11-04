# Minimum-area convex lattice polygons

Programs and data for the minimum area of a convex lattice polygon with n
vertices: OEIS [A070911](https://oeis.org/A070911) (twice the minimum area) and
[A063984](https://oeis.org/A063984) (the minimum number of interior lattice
points, (A070911(n) − n + 2)/2).

## Programs

Three Rust programs (standard library; `heuristic/` also uses `rayon`):

- **`certify/`** (`lattice-layered`): certified minima for every n up to a
  chosen largest one, in one run. A dynamic program over
  convex lattice chains bounded by lattice width, filled in order of the number
  of chain vertices, with Pareto frontiers of (direction, area), per-count area
  caps from feasible polygons, and a lower bound on the area in terms of the
  lattice width to stop. Every reported polygon is reconstructed and validated.
- **`modular/`** (`lattice-parametric-dp`): fast feasible polygons (upper
  bounds). The exact minimum over polygons whose edge vectors lie in a square
  of half-side ⌈√(2n/3)⌉, computed by a dynamic program that keeps the number
  of edges only modulo a small modulus, with a penalty on the number of edges.
  Small memory, one n per run.
- **`heuristic/`** (`lattice-classic-parallel`): the same restricted problem
  with the full count index, multi-threaded; `--hybrid` computes every n that
  shares one square in a single run, with two tables in
  memory and the rest streamed to disk.

## Data

[`data/`](data/README.md) holds the certified values of both sequences for
3 ≤ n ≤ 511, with one validated optimal polygon each, and, separately in
`data/conjectured/`, heuristic upper bounds (not proven minimal) for n ≤ 1000.

## Reproducing the results

Requires Rust (cargo) and Ruby. From the repository root:

```sh
scripts/build                              # builds all three programs
scripts/certified 300 --spill /big/disk/spill   # certified values, 3 ≤ n ≤ 300
scripts/conjectured 512 1000               # heuristic values, 512 ≤ n ≤ 1000
```

`scripts/certified LARGEST` computes upper bounds with `modular/`, runs
`certify/` once for all n ≤ LARGEST, writes `reproduced/b070911.txt`,
`reproduced/b063984.txt` and `reproduced/a070911_polygons.txt` with every
polygon re-validated, and compares them with `data/`. The spill directory holds
finished layers for polygon reconstruction: about 23 GB for n ≤ 300 and
337 GB for n ≤ 511; it is removed at the end.

`scripts/conjectured FIRST LAST` runs `modular/` as parallel single-threaded
jobs (largest first, within `--memory-cap-mib`, default 14336), writes the
`*_conjectured.txt` tables to `reproduced/`, and compares them with
`data/conjectured/`.

Measured on a 20-thread desktop CPU (Intel Core Ultra 7 265):

| Run | Time | Memory | Disk |
|---|---:|---:|---:|
| certified, n ≤ 300 | about 20 minutes | 1 GB | 23 GB |
| certified, n ≤ 511 | about 10 hours | 7.5 GB | 337 GB |
| conjectured, 512 ≤ n ≤ 1000 | several hours | under 14 GB | none |

Each program also runs on its own; `--help` lists its options, and
`cargo test --release` in each directory runs its tests.

## License

MIT; see [LICENSE](LICENSE).
