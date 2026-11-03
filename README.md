# Minimum-area convex lattice polygons

Programs that compute, for each k, the minimum area of a convex lattice
polygon with k vertices: OEIS [A070911](https://oeis.org/A070911) (twice the
minimum area) and [A063984](https://oeis.org/A063984) (the minimum number of
interior lattice points, (A070911(k) − k + 2)/2).

Two Rust programs, standard library plus `rayon` for the first:

- **`heuristic/`** (`lattice-classic-parallel`): fast upper bounds. An exact
  dynamic program over k-gons whose edge vectors lie in a box of half-side
  ⌈√(2k/3)⌉. Gives feasible polygons, not proofs of optimality.
- **`certify/`** (`lattice-layered`): certified minima for all k ≤ K in one
  run. A dynamic program over convex lattice chains by lattice width, filled
  in order of the number of chain vertices, with Pareto frontiers of
  (direction, area) and a lower bound on the area in terms of lattice width to
  stop. Every reported polygon is reconstructed and validated.

## Data

[`data/`](data/README.md) contains the certified values of both sequences for
3 ≤ k ≤ 511 and one validated optimal polygon for every k.

## Usage

```sh
cargo build --release --manifest-path heuristic/Cargo.toml
cargo build --release --manifest-path certify/Cargo.toml

K=200
for k in $(seq 3 $K); do
  area=$(heuristic/target/release/lattice-classic-parallel --k $k --threads 8 --memory-mib 16384 |
         grep -o '"doubled_area":[0-9]*' | cut -d: -f2)
  echo "$k $area"
done > upper.txt

certify/target/release/lattice-layered --k $K --upper upper.txt --spill ./spill --save polygons
```

The certified run prints one JSON line with all doubled areas from k=3 and
writes one polygon per k to `polygons/kNNNNNN.txt` (header, vertex count,
vertices). The upper bounds only need to be feasible; they limit the search.
`--spill DIR` holds finished layers on disk for witness reconstruction (about
23 GB at K=300, growing like K⁵) and a checkpoint: rerun with `--resume` after
an interruption. `--help` lists all options; `cargo test --release` runs the
tests in each directory.

Measured cost of the certified run on a 20-thread desktop CPU: K=300 in
17 minutes with 1 GB of RAM; K=511 in 9.5 hours with 7.5 GB of RAM and
337 GB of spill.

## License

MIT; see [LICENSE](LICENSE).
