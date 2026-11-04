//! S-parameters over frequency, in one shape for every reader: the session's sweep and time
//! domain, the analysis history and its project file, the charts and their comparisons.
//!
//! [`SData`] holds, at each frequency (Hz, ascending), the S entries of `outputs` rows for the
//! excited entries `inputs` (columns): every entry for a driven sweep (S is n × n), entry 0 alone
//! for the time domain (its S11 and S21). The numbers are flat, frequency by frequency, row by
//! row: S\[p\]\[inputs\[c\]\] at frequency k is at (k·outputs + p)·inputs + c.

use emcraft_solver::Complex64 as C;

/// Most frequencies of one set (a sweep's cap).
pub const MAX_FREQUENCIES: usize = emcraft_solver::driven::MAX_FREQUENCIES;

/// Most entries (rows, columns) of S.
pub const MAX_ENTRIES: usize = 64;

/// Most complex numbers of one set: 32 MB of them.
pub const MAX_VALUES: usize = 1 << 21;

/// S-parameters at a list of frequencies (see the module docs).
///
/// ```
/// use emcraft_engine::SData;
/// use emcraft_solver::Complex64 as C;
///
/// // A matched two-port at two frequencies: S21 = S12 = 1 there.
/// let s = |k: f64| [C::new(0.0, 0.0), C::new(k, 0.0), C::new(k, 0.0), C::new(0.0, 0.0)];
/// let data = SData::square(vec![1e9, 2e9], 2, s(1.0).into_iter().chain(s(0.5)).collect()).unwrap();
/// assert_eq!(data.get(1, 1, 0), Some(C::new(0.5, 0.0)));
/// assert_eq!(data.curve(1, 0).map(|(f, s)| (f, s.re)).collect::<Vec<_>>(), [(1e9, 1.0), (2e9, 0.5)]);
/// // Sizes that do not agree, or frequencies out of order, are refused.
/// assert!(SData::square(vec![1e9], 2, vec![C::new(0.0, 0.0); 3]).is_err());
/// assert!(SData::square(vec![2e9, 1e9], 1, vec![C::new(0.0, 0.0); 2]).is_err());
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct SData {
    frequencies: Vec<f64>,
    outputs: usize,
    inputs: Vec<usize>,
    s: Vec<C>,
}

impl SData {
    /// S at `frequencies` (Hz) with `outputs` rows for the excited entries `inputs`: an error
    /// saying what is wrong when the sizes do not agree or exceed their caps, a number is not
    /// finite or the frequencies are not positive and ascending (a file's are hostile).
    pub fn new(frequencies: Vec<f64>, outputs: usize, inputs: Vec<usize>, s: Vec<C>) -> Result<SData, String> {
        if frequencies.len() > MAX_FREQUENCIES {
            return Err(format!("more than {MAX_FREQUENCIES} frequencies"));
        }
        if frequencies.iter().any(|x| !(x.is_finite() && *x > 0.0)) || frequencies.windows(2).any(|w| w[1] < w[0]) {
            return Err("frequencies must be finite, positive and ascending".into());
        }
        if outputs == 0 || outputs > MAX_ENTRIES || inputs.is_empty() || inputs.len() > outputs || inputs.iter().any(|q| *q >= outputs) {
            return Err(format!("S has from 1 to {MAX_ENTRIES} entries, its excited ones among them"));
        }
        let per = outputs * inputs.len();
        if frequencies.len().saturating_mul(per) > MAX_VALUES {
            return Err(format!("S-parameters of more than {MAX_VALUES} values"));
        }
        if s.len() != frequencies.len() * per {
            return Err(format!("{} frequencies of {per} values each are {}, not {}", frequencies.len(), frequencies.len() * per, s.len()));
        }
        if !s.iter().all(|c| c.re.is_finite() && c.im.is_finite()) {
            return Err("S-parameters must be finite numbers".into());
        }
        Ok(SData { frequencies, outputs, inputs, s })
    }

    /// A whole S of `n` entries (every one excited).
    pub fn square(frequencies: Vec<f64>, n: usize, s: Vec<C>) -> Result<SData, String> {
        SData::new(frequencies, n, (0..n).collect(), s)
    }

    /// The frequencies (Hz), ascending.
    pub fn frequencies(&self) -> &[f64] {
        &self.frequencies
    }

    /// S's rows (its entries).
    pub fn outputs(&self) -> usize {
        self.outputs
    }

    /// The excited entries (S's columns).
    pub fn inputs(&self) -> &[usize] {
        &self.inputs
    }

    /// The values, flat (see the module docs).
    pub fn values(&self) -> &[C] {
        &self.s
    }

    /// The values at frequency `k`: `outputs` rows of one value per excited entry.
    pub fn at(&self, k: usize) -> &[C] {
        let per = self.outputs * self.inputs.len();
        self.s.get(k * per..(k + 1) * per).unwrap_or(&[])
    }

    /// S\[p\]\[q\] at frequency `k`; none when `q` is not excited or out of range.
    pub fn get(&self, k: usize, p: usize, q: usize) -> Option<C> {
        let c = self.inputs.iter().position(|x| *x == q)?;
        (p < self.outputs).then(|| self.at(k).get(p * self.inputs.len() + c).copied())?
    }

    /// S\[p\]\[q\] at each frequency (Hz); nothing when it is not held.
    pub fn curve(&self, p: usize, q: usize) -> impl Iterator<Item = (f64, C)> + '_ {
        (0..self.frequencies.len()).filter_map(move |k| Some((self.frequencies[k], self.get(k, p, q)?)))
    }
}

/// The time domain's S-parameters: a row per port, a column per port excited.
pub fn of_time_domain(t: &emcraft_fdtd::TdResult) -> Result<SData, String> {
    if !t.consistent() {
        return Err("the time domain's S-parameters are not one value a frequency, port and port excited".into());
    }
    let ni = t.inputs.len();
    let s = (0..t.frequencies.len()).flat_map(|k| (0..t.ports * ni).map(move |i| t.column(i % ni, i / ni)[k])).collect();
    SData::new(t.frequencies.clone(), t.ports, t.inputs.clone(), s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_column_holds_its_entries_and_no_other() {
        // The time domain's: S11 and S21 for entry 0 excited.
        let td = SData::new(vec![1e9, 2e9], 2, vec![0], vec![C::new(0.1, 0.0), C::new(0.9, 0.0), C::new(0.2, 0.0), C::new(0.8, 0.0)]).unwrap();
        assert_eq!(td.get(1, 1, 0), Some(C::new(0.8, 0.0)));
        assert_eq!(td.get(0, 0, 1), None);
        assert_eq!(td.get(0, 2, 0), None);
        assert_eq!(td.get(2, 0, 0), None);
        assert_eq!(td.curve(0, 0).count(), 2);
    }

    #[rstest::rstest]
    #[case::no_entries(0, vec![0])]
    #[case::none_excited(2, vec![])]
    #[case::excited_out_of_range(2, vec![2])]
    #[case::too_many(MAX_ENTRIES + 1, vec![0])]
    fn shapes_that_do_not_fit_are_refused(#[case] outputs: usize, #[case] inputs: Vec<usize>) {
        assert!(SData::new(vec![1e9], outputs, inputs.clone(), vec![C::new(0.0, 0.0); outputs * inputs.len()]).is_err());
    }

    #[test]
    fn numbers_that_are_not_finite_are_refused() {
        assert!(SData::square(vec![1e9], 1, vec![C::new(f64::NAN, 0.0)]).is_err());
        assert!(SData::square(vec![f64::INFINITY], 1, vec![C::new(0.0, 0.0)]).is_err());
        assert!(SData::square(vec![0.0], 1, vec![C::new(0.0, 0.0)]).is_err());
    }
}
