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

## Conjectured values (`conjectured/`), NOT proven minimal

- `b070911_conjectured.txt`, `b063984_conjectured.txt`,
  `a070911_conjectured_polygons.txt`: n = 512..1000.
- `b070911_heuristic.txt`, `b063984_heuristic.txt`,
  `a070911_heuristic_polygons.txt`: the full heuristic sequence, n = 3..1000.

Each value is the doubled area of an explicit, validated convex lattice n-gon,
so it is a proven **upper bound**. It is the exact minimum over polygons whose
edge vectors lie in [-s,s]^2, s = ceil(sqrt(2n/3)) (computed with `heuristic/`
or the modular-penalty solver), not over all polygons. For n <= 511 it equals
the certified minimum for 484 of 509 counts, including every n >= 296; the 25
exceptions (n = 211 and 272..295) exceed the minimum by at most 0.121%. Most of
the values for n > 511 are probably optimal (we expect more than 90%), but some
may be slightly too large. Each file repeats this remark in its header.
