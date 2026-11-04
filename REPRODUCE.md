# Reproducing the results

## What we used

- **Machine:** a desktop with an Intel Core Ultra 7 265 (20 logical CPUs:
  8 performance and 12 efficiency cores), Linux, and an 8 TB hard disk for the
  spill directory. All runs below were done on it.
- **Software:** Rust (cargo) 1.98 and Ruby 3.3. Any recent stable Rust with
  edition 2024 (Rust 1.85 or later) and Ruby 3.0 or later should work. The
  programs use only the Rust standard library, plus `rayon` in `heuristic/`.

## Compile

```sh
git clone https://github.com/sarielhp/26-11-min-area-lattice-polygons
cd 26-11-min-area-lattice-polygons
scripts/build
```

`scripts/build` runs `cargo build --release --locked` in `certify/`,
`heuristic/` and `modular/`. To check the programs, run `cargo test --release`
in each of these directories.

## Run

| Command | Reproduces | Time | Memory | Disk |
|---|---|---:|---:|---:|
| `scripts/certified 300 --spill DIR` | certified values, 3 ≤ n ≤ 300 | about 20 minutes | 1 GB | 23 GB |
| `scripts/certified 511 --spill DIR` | certified values, 3 ≤ n ≤ 511 (`data/`) | about 10 hours | 7.5 GB | 337 GB |
| `scripts/conjectured 512 1000` | conjectured values (`data/conjectured/`) | several hours | under 14 GB | none |
| `scripts/conjectured 3 1000` | the full heuristic sequence | several hours | under 14 GB | none |

Times are for the machine above with all 20 threads. `DIR` should be on a
large disk (not a RAM disk); it holds the finished layers of the certificate
for reconstructing the polygons and is removed at the end.

Each script writes its tables to `reproduced/` (change with `--out DIR`),
re-validates every polygon, and ends by comparing its values with the
published ones in `data/`, printing how many differ (expected: 0).

Useful options: `--threads T` (certificate threads), `--jobs J` and
`--memory-cap-mib M` (parallel heuristic jobs and their total memory; the
default is 14336 MiB). The individual programs also run on their own; see
`--help` for each.

## What to expect

- `scripts/certified` first computes heuristic upper bounds for every n (a few
  minutes for n ≤ 511), then runs the certificate once. It prints one line per
  n as each value becomes final, roughly in increasing order of n.
- `scripts/conjectured` prints one line per n as each job finishes, largest n
  first. Running times vary by a factor of a few between neighbouring n: some
  n need a larger internal modulus, odd n take longer, and the efficiency cores
  are slower.
