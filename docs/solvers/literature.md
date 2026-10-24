# Literature

The methods EMCraft implements and where they come from.

## Elements and discretisation
- H. Whitney, *Geometric Integration Theory*, Princeton University Press 1957 — the lowest-order
  edge functions λ_i∇λ_j − λ_j∇λ_i (`emcraft-fem::nedelec`, order 1).
- J.-C. Nédélec, *Mixed finite elements in R³*, Numer. Math. 35:315–341, 1980 — the
  H(curl)-conforming spaces of the first kind.
- J. P. Webb, *Hierarchal vector basis functions of arbitrary order for triangular and
  tetrahedral finite elements*, IEEE Trans. Antennas Propag. 47(8):1244–1253, 1999 — the order 2
  basis (edge gradients ∇(λ_iλ_j), face functions λ_i w_jk).
- Z. J. Cendes, *Vector finite elements for electromagnetic field computation*, IEEE Trans. Magn.
  27(5):3958–3966, 1991 — element matrices from barycentric integration.
- J.-M. Jin, *The Finite Element Method in Electromagnetics*, 3rd ed., Wiley 2014 — the
  curl–curl formulation, waveguide port conditions with modal expansions (§8.5, §9.2).
- H. W. Kuhn, *Some combinatorial lemmas in topology*, IBM J. Res. Dev. 4(5):518–524, 1960 — the
  six-tetrahedra split of a cube (`emcraft-mesh`).

## Waveguides and references
- D. M. Pozar, *Microwave Engineering*, 4th ed., Wiley 2012 — rectangular waveguide modes (§3.3),
  S-parameters and power waves (§4.3–4.4).
- R. E. Collin, *Foundations for Microwave Engineering*, 2nd ed., McGraw-Hill 1992 — evanescent
  modes and transmission-line equivalents (§3.12, §4.2).

## Sparse direct solution
- A. George, *Nested dissection of a regular finite element mesh*, SIAM J. Numer. Anal.
  10(2):345–363, 1973; H. D. Simon, *Partitioning of unstructured problems for parallel
  processing*, Computing Systems in Engineering 2:135–148, 1991 — the ordering.
- J. W. H. Liu, *The role of elimination trees in sparse factorization*, SIAM J. Matrix Anal.
  Appl. 11(1):134–172, 1990 — elimination trees and postorders.
- I. S. Duff, J. K. Reid, *The multifrontal solution of indefinite sparse symmetric linear
  equations*, ACM TOMS 9(3):302–325, 1983; J. W. H. Liu, *The multifrontal method for sparse
  matrix solution: theory and practice*, SIAM Review 34(1):82–109, 1992 — the multifrontal
  method.
- C. Ashcraft, R. Grimes, *The influence of relaxed supernode partitions on the multifrontal
  method*, ACM TOMS 15(4):291–309, 1989; Y. Chen, T. A. Davis, W. W. Hager, S. Rajamanickam,
  *Algorithm 887: CHOLMOD*, ACM TOMS 35(3):22, 2008 — relaxed amalgamation and its thresholds.
- J. R. Bunch, L. Kaufman, *Some stable methods for calculating inertia and solving symmetric
  linear systems*, Math. Comp. 31(137):163–179, 1977 — 1 × 1 and 2 × 2 pivots.
- O. Schenk, K. Gärtner, *Solving unsymmetric sparse systems of linear equations with PARDISO*,
  FGCS 20(3):475–487, 2004 — static pivoting with iterative refinement.

## Quadrature
- A. H. Stroud, *Approximate Calculation of Multiple Integrals*, Prentice-Hall 1971; M. G. Duffy,
  *Quadrature over a pyramid or cube of integrands with a singularity at a vertex*, SIAM J.
  Numer. Anal. 19(6):1260–1262, 1982 — collapsed (conical product) rules on simplices.
