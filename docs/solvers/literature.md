# Literature

The methods EMCraft implements and where they come from.

## Meshing
The tetrahedral mesher (`emcraft-tetmesh`) is FieldCraft's; its modules cite their sources. In
short: J. R. Shewchuk, *Adaptive precision floating-point arithmetic and fast robust geometric
predicates*, Discrete Comput. Geom. 18(3):305–363, 1997; A. Bowyer, *Computing Dirichlet
tessellations*, and D. F. Watson, *Computing the n-dimensional Delaunay tessellation*, Comput.
J. 24(2), 1981; L. Diazzi, D. Panozzo, A. Vaxman, M. Attene, *Constrained Delaunay
tetrahedrization: a robust and practical approach*, ACM TOG 42(6), 2023; J. Tournois, C.
Wormser, P. Alliez, M. Desbrun, *Interleaving Delaunay refinement and optimization for practical
isotropic tetrahedron mesh generation*, ACM TOG 28(3), 2009; B. M. Klingner, J. R. Shewchuk,
*Aggressive tetrahedral mesh improvement*, Proc. 16th International Meshing Roundtable, 2007.
- S. W. Sloan, *A fast algorithm for generating constrained Delaunay triangulations*, Computers
  & Structures 47(3):441–450, 1993 — recovering the intersection segments in each triangle a
  Boolean splits, by flipping the edges that cross them (`emcraft-mesh::csg`).
- A. Van Oosterom, J. Strackee, *The solid angle of a plane triangle*, IEEE Trans. Biomed. Eng.
  30(2):125–126, 1983 — the winding number that classifies a Boolean's pieces inside or outside
  the other surface (`emcraft-mesh::csg`).

## Special functions
- M. Abramowitz, I. A. Stegun, *Handbook of Mathematical Functions*, 1964, §9.12 and Tables 9.5,
  10.6 — Bessel functions by backward recurrence, their zeros (`emcraft-fem::bessel`, used by
  `emcraft-theory::bessel` and the circular ports).
- R. F. Harrington, *Time-Harmonic Electromagnetic Fields*, McGraw-Hill 1961, §6.10 — the
  resonances of a spherical cavity.

## Elements and discretisation
- H. Whitney, *Geometric Integration Theory*, Princeton University Press 1957 — the lowest-order
  edge functions λ_i∇λ_j − λ_j∇λ_i (`emcraft-fem::nedelec`, order 1).
- J.-C. Nédélec, *Mixed finite elements in R³*, Numer. Math. 35:315–341, 1980 — the
  H(curl)-conforming spaces of the first kind.
- P. Monk, *Finite Element Methods for Maxwell's Equations*, Oxford 2003, §3.9 — the covariant
  Piola transform N = DΦ^{−T} N̂, curl N = DΦ curl N̂ / det DΦ that carries edge elements to
  curved (isoparametric) tetrahedra (`emcraft-fem::nedelec`, `Element::curved`).
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
- J.-F. Lee, D.-K. Sun, Z. J. Cendes, *Full-wave analysis of dielectric waveguides using
  tangential vector finite elements*, IEEE Trans. MTT 39(8):1262–1271, 1991; J.-M. Jin, *The
  Finite Element Method in Electromagnetics*, 3rd ed., Wiley 2014, §8.4 — the modes of a port of
  any cross-section: edge functions for the transverse field, nodal ones for the axial, the
  generalised eigenproblem in −β² (`emcraft-solver::portmode`, elements in `emcraft-fem::tri`).
- R. E. Collin, *Field Theory of Guided Waves*, 2nd ed., IEEE Press 1991, §6.1; R. F.
  Harrington, *Time-Harmonic Electromagnetic Fields*, McGraw-Hill 1961, §4-7 — the LSE and LSM
  modes of a slab-loaded rectangular guide, the reference for hybrid port modes
  (`emcraft-theory::slab`).
- E. Hammerstad, Ø. Jensen, *Accurate models for microstrip computer-aided design*, IEEE MTT-S
  International Microwave Symposium Digest, 1980, pp. 407–409 — microstrip's quasi-static
  effective permittivity and impedance, the benchmark for strips on a substrate
  (`emcraft-theory::microstrip`).
- S. B. Cohn, *Characteristic impedance of the shielded-strip transmission line*, IRE Trans. MTT
  2(2):52–57, 1954 — the zero-thickness stripline's exact impedance (`emcraft-theory::stripline`).
- K. Kurokawa, *Power waves and the scattering matrix*, IEEE Trans. MTT 13(2):194–202, 1965 — S
  referred to reference impedances from the modes' power waves (`emcraft-solver::driven`).
- J. Meixner, *The behavior of electromagnetic fields at edges*, IEEE Trans. AP 20(4):442–446,
  1972 — E ∝ r^{−1/2} at a strip's edge, why the structured mesh closes in harder there than at
  an iris' (`emcraft-mesh`, `strip_edge`).

## Discontinuities
- A. Wexler, *Solution of waveguide discontinuities by modal analysis*, IEEE Trans. MTT
  15(9):508–517, 1967; T. Itoh (ed.), *Numerical Techniques for Microwave and Millimeter-Wave
  Passive Structures*, Wiley 1989, ch. 9; J. Uher, J. Bornemann, U. Rosenberg, *Waveguide
  Components for Antenna Feed Systems: Theory and CAD*, Artech House 1993 — mode matching and
  generalised scattering matrices (`emcraft-theory::modematch`).
- R. Mittra, T. Itoh, T.-S. Li, *Analytical and numerical studies of the relative convergence
  phenomenon arising in the solution of an integral equation by the moment method*, IEEE Trans.
  MTT 20(2):96–104, 1972 — mode counts in proportion to the apertures.
- J. Meixner, *The behavior of electromagnetic fields at edges*, IEEE Trans. Antennas Propag.
  20(4):442–446, 1972 — the r^{-1/3} field at a thin edge; I. Babuška, R. B. Kellogg, J.
  Pitkäranta, *Direct and inverse error estimates for finite elements with mesh refinements*,
  Numer. Math. 33:447–471, 1979 — geometric grading towards singularities (`emcraft-mesh`).

## Losses
- T. B. A. Senior, J. L. Volakis, *Approximate Boundary Conditions in Electromagnetics*, IEE 1995,
  §2.2 — the Leontovich surface-impedance condition for walls of finite conductivity.
- D. M. Pozar, *Microwave Engineering*, 4th ed., Wiley 2012, eq. 3.96 — TE10 conductor
  attenuation (the reference); §1.3 — the loss tangent.

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
- A. van der Sluis, *Condition numbers and equilibration of matrices*, Numer. Math. 14:14–23,
  1969 — the symmetric scaling S A S, each row's largest entry one, before the factorization:
  the static-pivot threshold then means the same for every unknown of a graded mesh.

## Time domain
- K. S. Yee, *Numerical solution of initial boundary value problems involving Maxwell's
  equations in isotropic media*, IEEE Trans. Antennas Propag. 14(3):302–307, 1966; T. Weiland,
  *A discretization method for the solution of Maxwell's equations for six-component fields*,
  AEÜ 31:116–120, 1977 — the scheme and its integral (FIT) form on non-uniform grids.
- J. A. Roden, S. D. Gedney, *Convolution PML (CPML): an efficient FDTD implementation of the
  CFS–PML for arbitrary media*, Microw. Opt. Technol. Lett. 27(5):334–339, 2000; A. Taflove, S.
  C. Hagness, *Computational Electrodynamics: The FDTD Method*, 3rd ed., Artech House 2005,
  ch. 7 — the absorbing layers and their grading.

## Eigenvalues
- K.-J. Bathe, E. L. Wilson, *Large eigenvalue problems in dynamic analysis*, J. Eng. Mech. Div.
  ASCE 98(6):1471–1485, 1972; K.-J. Bathe, *Finite Element Procedures*, 2nd ed. 2014, §11.6 —
  subspace iteration with Rayleigh–Ritz.
- G. H. Golub, C. F. Van Loan, *Matrix Computations*, 4th ed., Johns Hopkins 2013, §8.5 — the
  cyclic Jacobi method for the reduced problem; §7.4–7.6 — Householder reduction to Hessenberg
  form, the Francis double-shift QR algorithm and inverse iteration, for the nonsymmetric
  projected problems of the port modes (`emcraft-solver::dense`).
- D. M. Pozar, *Microwave Engineering*, 4th ed., Wiley 2012, §6.3 and §6.7 — cavity resonances,
  Q by perturbation, the TE101 Q (eq. 6.46).

## Open problems
- Z. S. Sacks, D. M. Kingsland, R. Lee, J.-F. Lee, *A perfectly matched anisotropic absorber for
  use as an absorbing boundary condition*, IEEE Trans. Antennas Propag. 43(12):1460–1463, 1995;
  J.-M. Jin, *The Finite Element Method in Electromagnetics*, 3rd ed., §9.6 — the layer as the
  medium εΛ, μΛ of stretched coordinates (`emcraft-solver::driven`, `add_pml`).
- C. A. Balanis, *Advanced Engineering Electromagnetics*, 2nd ed., Wiley 2012, §6.8 — the
  near-to-far transform of equivalent currents; *Antenna Theory*, 4th ed., §4.2 — the Hertzian
  dipole (`emcraft-solver::radiation`, `emcraft-theory::radiation`).
- J.-M. Jin, *The Finite Element Method in Electromagnetics*, 3rd ed., §9.4 — lumped ports as
  currents across a gap behind an impedance; C. A. Balanis, *Antenna Theory*, §8.5 — the
  induced-EMF impedance of a dipole and the sine and cosine integrals (Abramowitz & Stegun
  §5.2) (`emcraft-solver::driven`, `emcraft-theory::radiation`).
- M. Piket-May, A. Taflove, J. Baron, *FD-TD modeling of digital signal propagation in 3-D
  circuits with passive and active loads*, IEEE Trans. MTT 42(8):1514–1523, 1994 — resistive
  voltage sources (lumped ports) on Yee edges; K. R. Umashankar, A. Taflove, B. Beker, IEEE
  Trans. AP 35(11), 1987 — thin wires as lines of edges (`emcraft-fdtd::open`).
- C. F. Bohren, D. R. Huffman, *Absorption and Scattering of Light by Small Particles*, Wiley
  1983, §4.4 and Appendix A — the Mie series and its BHMIE test values
  (`emcraft-theory::radiation`).

## Quadrature
- A. H. Stroud, *Approximate Calculation of Multiple Integrals*, Prentice-Hall 1971; M. G. Duffy,
  *Quadrature over a pyramid or cube of integrands with a singularity at a vertex*, SIAM J.
  Numer. Anal. 19(6):1260–1262, 1982 — collapsed (conical product) rules on simplices.
