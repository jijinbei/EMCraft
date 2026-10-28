# A post from a SolidCraft design, its diameter a parameter

`post.solidcraft` is a SolidCraft design: a 22.86 × 10.16 × 30 mm guide (parameters `a`, `b`,
`l`) with a full-height post of diameter `d` (3 mm as saved) cut across it at its middle. It was
written with SolidCraft's document API by `make-post.rs.txt` (kept here as text). The case opens
it with `d` set to 4 mm (`geometry.import` `parameters`), so SolidCraft evaluates the design at
that diameter and hands over each face's exact surface through its exchange snapshot
(`solidcraft-exchange`, docs/adr/0003-solidcraft-exchange.md), then solves it by the same
adaptive recipe as `imported-post`. The reference is the closed-form post of emcraft-theory at
4 mm, computed separately from the finite elements.
