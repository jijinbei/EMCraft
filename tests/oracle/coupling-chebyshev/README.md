# Coupling matrices: synthesis against the g-values, extraction against synthesis

- `filter.synthesize` makes the folded coupling matrix of a fifth-order Chebyshev filter with
  20 dB return loss. An all-pole filter folds to a line whose couplings are those of the
  low-pass prototype, M_k,k+1 = 1/√(g_k g_k+1) (Matthaei, Young & Jones 1964), the g-values
  from the ripple by the closed form, evaluated independently.
- A fourth-order quasi-elliptic filter (22 dB, zeros at ω = ±1.8) is synthesized and its
  band-pass response kept; `results.coupling` extracts it again from |S| alone: the zeros come
  back and the model's |S21| is the response's.
