# Data

In every table the first column is the number of vertices. (The OEIS calls
it n; the files follow the OEIS b-file format, one "number value" pair per
line, with comment lines starting with #.)

## Certified values, 3 to 511 vertices

- `b070911.txt`: twice the minimum area of a convex lattice polygon with the
  given number of vertices (OEIS A070911).
- `b063984.txt`: the minimum number of interior lattice points (OEIS A063984),
  equal to (twice the area − number of vertices + 2)/2.
- `a070911_polygons.txt`: one optimal polygon per number of vertices, as
  `vertices doubled-area (x1,y1),(x2,y2),...`, vertices in counterclockwise
  order.

The values up to 150 vertices agree with the OEIS. They come from two
certified runs of `certify/`, up to 300 and up to 511 vertices, using upper
bounds from the heuristic; the two runs agree up to 300 vertices. Every polygon
was re-validated independently of the solver: vertex count, strict convexity,
and shoelace area.

## Conjectured values (`conjectured/`), not proven minimal

- `b070911_conjectured.txt`, `b063984_conjectured.txt`,
  `a070911_conjectured_polygons.txt`: 512 to 1000 vertices.
- `b070911_heuristic.txt`, `b063984_heuristic.txt`,
  `a070911_heuristic_polygons.txt`: the full heuristic sequence, 3 to 1000
  vertices.

Each value is the doubled area of an explicit, validated convex lattice
polygon, so it is a proven **upper bound**. It is the exact minimum over
polygons whose edge vectors lie in a square of half-side
⌈√(2 × vertices / 3)⌉ (computed with `modular/` or `heuristic/`), not over all
polygons. Up to 511 vertices it equals the certified minimum for 484 of the 509
cases, including every case from 296 vertices on; the 25 exceptions (211 and
272 to 295 vertices) exceed the minimum by at most 0.121%. Most of the values
beyond 511 vertices are probably optimal (we expect more than 90%), but some
may be slightly too large. Each file repeats this remark in its header.
