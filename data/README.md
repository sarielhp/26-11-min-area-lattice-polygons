# Data

In every table the first column is n, the number of vertices. The files
follow the OEIS b-file format: one "n value" pair per line, comment lines
starting with #.

## Certified values, 3 ≤ n ≤ 511

- `b070911.txt`: twice the minimum area of a convex lattice n-gon (OEIS
  A070911).
- `b063984.txt`: the minimum number of interior lattice points (OEIS A063984),
  equal to (A070911(n) − n + 2)/2.
- `a070911_polygons.txt`: one optimal polygon per n, as
  `n doubled-area (x1,y1),(x2,y2),...`, vertices in counterclockwise order.

The values for n ≤ 150 agree with the OEIS. They come from two certified runs
of `certify/`, for n ≤ 300 and n ≤ 511, using upper bounds from the heuristic;
the two runs agree for n ≤ 300. Every polygon
was re-validated independently of the solver: vertex count, strict convexity,
and shoelace area.

## Conjectured values (`conjectured/`), not proven minimal

- `b070911_conjectured.txt`, `b063984_conjectured.txt`,
  `a070911_conjectured_polygons.txt`: 512 ≤ n ≤ 1000.
- `b070911_heuristic.txt`, `b063984_heuristic.txt`,
  `a070911_heuristic_polygons.txt`: the full heuristic sequence, 3 ≤ n ≤ 1000.

Each value is the doubled area of an explicit, validated convex lattice
polygon, so it is a proven **upper bound**. It is the exact minimum over
polygons whose edge vectors lie in a square of half-side
⌈√(2n/3)⌉ (computed with `modular/` or `heuristic/`), not over all
polygons. For n ≤ 511 it equals the certified minimum for 484 of the 509 values,
including every n ≥ 296; the 25 exceptions (n = 211 and 272 ≤ n ≤ 295) exceed the minimum by at most 0.121%. Most of the values
for n > 511 are probably optimal (we expect more than 90%), but some
may be slightly too large. Each file repeats this remark in its header.
