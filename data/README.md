# Certified values, 3 ≤ n ≤ 511

- `b070911.txt`: `n V(n)`, twice the minimum area of a convex lattice n-gon
  (OEIS A070911).
- `b063984.txt`: `n I(n)`, the minimum number of interior lattice points,
  I(n) = (V(n) − n + 2)/2 (OEIS A063984).
- `a070911_polygons.txt`: one optimal polygon per n, as
  `n V(n) (x1,y1),(x2,y2),...`, vertices in order.

Values for n ≤ 150 agree with the OEIS. Values for n ≤ 300 come from a
certified run with K=300 and for n ≤ 511 from a run with K=511 (`certify/`,
using upper bounds from `heuristic/`); the two runs agree on n ≤ 300. Every
polygon was re-validated independently of the solver: vertex count, strict
convexity, and shoelace area.
