//! A frequency's system factored and solved for each excitation: the symbolic analysis shared
//! across frequencies, the solves with iterative refinement, and the result's counts and
//! timings, for the ports' symmetric systems (LDLᵀ) and the periodic cell's (LU) alike.

use std::borrow::Cow;
use std::time::Instant;

use emcraft_model::Point3;
use num_complex::Complex64 as C;

use super::assembly::System;
use super::{Driven, FrequencyResult, Result, Timings};
use crate::ldlt::{Analysis, FactorError, Ldlt, memory_budget};
use crate::lu::Lu;
use crate::smatrix::SMatrix;
use crate::sparse::Csr;

/// What a factorization tells a [`FrequencyResult`].
#[derive(Clone, Copy, Debug)]
pub(super) struct FactorReport {
    perturbed: usize,
    entries: usize,
    largest_front: usize,
    /// Seconds of the analysis (0 when an earlier frequency's was reused) and of the numeric
    /// factorization.
    analysis: f64,
    factor: f64,
}

/// A factorization a frequency's system is solved with.
pub(super) trait Factor {
    fn solve_refined(&self, a: &Csr, b: &[C], steps: usize) -> std::result::Result<(Vec<C>, f64), FactorError>;
    fn report(&self) -> FactorReport;
}

/// The LDLᵀ and the LU report alike.
macro_rules! factor {
    ($($t:ty),*) => {$(
        impl Factor for $t {
            fn solve_refined(&self, a: &Csr, b: &[C], steps: usize) -> std::result::Result<(Vec<C>, f64), FactorError> {
                <$t>::solve_refined(self, a, b, steps)
            }

            fn report(&self) -> FactorReport {
                FactorReport {
                    perturbed: self.perturbed,
                    entries: self.entries,
                    largest_front: self.largest_front,
                    analysis: self.analysis_seconds,
                    factor: self.numeric_seconds,
                }
            }
        }
    )*};
}

factor!(Ldlt, Lu);

impl Driven {
    /// The symbolic analysis of `a`, whose unknowns are at `points` and then its `border`
    /// (eliminated last): the one kept from an earlier frequency when it fits (the pattern does
    /// not change with the frequency), else a new one.
    fn analysis_of(&self, a: &Csr, points: &[Point3], border: usize) -> Result<Cow<'_, Analysis>> {
        if let Some(an) = self.analysis.get().filter(|an| an.fits(a)) {
            return Ok(Cow::Borrowed(an));
        }
        let points: Vec<Point3> = points.iter().copied().chain(std::iter::repeat_n(Point3::ORIGIN, border)).collect();
        Ok(Cow::Owned(Analysis::new(a, &points, border)?))
    }

    /// Factors `a` by `factor` with its analysis ([`Driven::analysis_of`]), a new analysis kept
    /// for the next frequencies (the first stays when two race) and its time reported only by the
    /// frequency that made it.
    fn factor_with<F>(&self, a: &Csr, points: &[Point3], border: usize, factor: impl Fn(&Analysis) -> Result<F>) -> Result<(F, f64)> {
        let an = self.analysis_of(a, points, border)?;
        let f = factor(&an)?;
        Ok(match an {
            Cow::Borrowed(_) => (f, 0.0),
            Cow::Owned(an) => {
                let seconds = an.seconds;
                let _ = self.analysis.set(an);
                (f, seconds)
            }
        })
    }

    /// Factors a [`System`], its border last.
    pub(crate) fn factor(&self, s: &System) -> Result<Ldlt> {
        let (mut f, analysis) = self.factor_with(&s.a, &self.dofs.points, s.border, |an| Ok(Ldlt::factor_analysed(&s.a, an, memory_budget())?))?;
        f.analysis_seconds = analysis;
        Ok(f)
    }

    /// Factors a periodic cell's bordered system `a` (unsymmetric) by LU, its unknowns at
    /// `points` and then its `border`.
    pub(super) fn factor_lu(&self, a: &Csr, points: &[Point3], border: usize) -> Result<Lu> {
        let (mut f, analysis) = self.factor_with(a, points, border, |an| Ok(Lu::factor_analysed(a, an, memory_budget())?))?;
        f.analysis_seconds = analysis;
        Ok(f)
    }
}

/// A frequency's system solved for each excitation.
pub(super) struct Solved {
    /// Each excitation's solution.
    pub fields: Vec<Vec<C>>,
    /// The worst backward error of the solves.
    pub backward_error: f64,
    report: FactorReport,
    /// Seconds of the system's assembly, and when the solves began.
    assembly: f64,
    started: Instant,
}

/// Solves `a` x = b for each of `rhs` with `f` and iterative refinement, keeping the first `keep`
/// unknowns of each solution (the border's are the ports' auxiliaries); `assembly`: the
/// seconds the system took.
pub(super) fn solve_each(f: &impl Factor, a: &Csr, rhs: &[Vec<C>], keep: usize, assembly: f64) -> Result<Solved> {
    let started = Instant::now();
    let mut fields = Vec::with_capacity(rhs.len());
    let mut worst: f64 = 0.0;
    for b in rhs {
        let (mut x, e) = f.solve_refined(a, b, 4)?;
        x.truncate(keep);
        worst = worst.max(e);
        fields.push(x);
    }
    Ok(Solved { fields, backward_error: worst, report: f.report(), assembly, started })
}

impl Solved {
    /// The result at `frequency` with the S-parameters and impedances read from the fields.
    pub fn result(self, frequency: f64, s: SMatrix, impedances: Vec<Option<f64>>) -> FrequencyResult {
        let r = self.report;
        FrequencyResult {
            frequency,
            s,
            impedances,
            fields: self.fields,
            backward_error: self.backward_error,
            perturbed_pivots: r.perturbed,
            factor_entries: r.entries,
            largest_front: r.largest_front,
            timings: Timings { assembly: self.assembly, analysis: r.analysis, factor: r.factor, solve: self.started.elapsed().as_secs_f64() },
        }
    }
}
