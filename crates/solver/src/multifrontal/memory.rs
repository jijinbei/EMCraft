//! The memory a factorization takes, planned from its [`Analysis`] before any front is made: the
//! fronts and update matrices alive at once in the order [`super::factor_tree`] makes them, and
//! the factor's columns kept in memory beside them. The plan picks the threads (each thread's
//! subtree holds fronts of its own) and the factor's share in memory so the two fit in the memory
//! the system has available; a factorization that cannot fit even on one thread is refused before
//! it starts, with what it needs, rather than taking the machine's memory until the system kills
//! the process.
//!
//! The working memory is followed as in the multifrontal method's stack (J. W. H. Liu, *On the
//! storage requirement in the out-of-core multifrontal method for sparse factorization*, ACM
//! TOMS 12(3):249–264, 1986): a front of m rows is m² entries, assembled while its children's
//! updates are still held; eliminated, it leaves its update for the parent and the columns the
//! factor keeps.

use std::cell::Cell;

use faer::Par;

use super::{Analysis, FactorError, Tree};

/// The share of the available memory a factorization plans to use: the rest is left for what
/// runs beside it (the matrices, the right-hand sides and fields, the allocator's slack).
const HEADROOM: f64 = 0.8;

/// The factor's memory when the system does not say what it has available (bytes).
const UNKNOWN_BUDGET: u64 = 8_000_000_000;

/// What a factorization's fronts leave behind, by its kernel: the entries of the update for the
/// parent (`update(m − k)`), the entries the factor keeps of a front (`kept(k, m)`), and whether
/// the factor may go to a scratch file on disk once the memory set for it is full.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Shape {
    pub update: fn(usize) -> usize,
    pub kept: fn(usize, usize) -> usize,
    pub out_of_core: bool,
}

/// How a factorization runs within the memory: on how many threads, how many bytes of the factor
/// it keeps in memory at most (the rest on disk), the bytes of fronts and updates it holds at
/// most, and the factor's bytes in all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Plan {
    pub threads: usize,
    pub in_memory: u64,
    pub working: u64,
    pub factor: u64,
}

/// The memory the system reports available (bytes; Linux's `MemAvailable`), `None` elsewhere.
pub(crate) fn available_memory() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = text.lines().find(|l| l.starts_with("MemAvailable:"))?;
    line.split_whitespace().nth(1)?.parse::<u64>().ok().map(|kb| kb.saturating_mul(1024))
}

impl Plan {
    /// The plan for factoring `an` with fronts of `shape`, at most `budget` bytes of the factor in
    /// memory and `available` bytes of memory (`None`: not known, then all the threads and the
    /// factor's memory capped at 8 GB), on `threads` threads or fewer: the most threads whose
    /// fronts fit, the factor's memory what they leave. Refused when the fronts do not fit on one
    /// thread, or (a factor kept in memory) the factor does not fit beside them.
    ///
    /// The plan follows the factorization's order exactly, so it bounds the memory the fronts
    /// take whatever the order the threads finish in: the light subtrees, each on a thread, hold
    /// at most the `threads` largest of their own peaks at once beside the updates the finished
    /// ones leave; the top then runs alone.
    pub(crate) fn new(an: &Analysis, shape: Shape, budget: u64, available: Option<u64>, threads: usize) -> Result<Plan, FactorError> {
        let tree = Tree::new(an);
        let factor = 16 * factor_entries(an, shape);
        if !shape.out_of_core && factor > budget {
            return Err(FactorError::FactorTooLarge { needs: factor, budget });
        }
        let Some(available) = available else {
            return Ok(Plan {
                threads: threads.max(1),
                in_memory: budget.min(UNKNOWN_BUDGET),
                working: 16 * working(an, &tree, shape, threads),
                factor,
            });
        };
        let usable = (available as f64 * HEADROOM) as u64;
        let mut t = threads.max(1);
        loop {
            let working = 16 * working(an, &tree, shape, t);
            let left = usable.saturating_sub(working);
            let fits = working <= usable && (shape.out_of_core || factor <= left);
            if fits {
                return Ok(Plan { threads: t, in_memory: left.min(budget), working, factor });
            }
            if t == 1 {
                let needs = if shape.out_of_core { working } else { working.saturating_add(factor) };
                return Err(FactorError::OutOfMemory { needs, available: usable });
            }
            t = t.div_ceil(2);
        }
    }
}

/// The entries the factor keeps of every front.
fn factor_entries(an: &Analysis, shape: Shape) -> u64 {
    (0..an.supernodes()).map(|s| (shape.kept)(an.columns(s).len(), an.super_rows[s].len()) as u64).fold(0u64, u64::saturating_add)
}

/// The most entries of fronts and updates alive at once factoring `an` on `threads` threads (see
/// [`Plan::new`]).
fn working(an: &Analysis, tree: &Tree, shape: Shape, threads: usize) -> u64 {
    let (tasks, top) = tree.split(threads);
    let update = |s: usize| (shape.update)(an.super_rows[s].len() - an.columns(s).len()) as u64;
    let mut peaks: Vec<u64> = tasks.iter().map(|&s| sequential(an, tree, shape, tree.first[s]..=s, 0).0).collect();
    peaks.sort_unstable_by(|a, b| b.cmp(a));
    let finished: u64 = tasks.iter().map(|&s| update(s)).sum();
    let parallel = finished.saturating_add(peaks.iter().take(threads.max(1)).sum());
    let (on_top, _) = sequential(an, tree, shape, top, finished);
    parallel.max(on_top)
}

/// The most entries alive factoring the supernodes `order` one after another, `held` entries of
/// updates held at the start; and those held at the end. A front's children's updates are
/// held until it is assembled; it is then m² entries, and eliminating it adds its update and a
/// copy of the columns the factor keeps before the front is freed.
fn sequential(an: &Analysis, tree: &Tree, shape: Shape, order: impl IntoIterator<Item = usize>, held: u64) -> (u64, u64) {
    let mut held = held;
    let mut peak = held;
    for s in order {
        let (k, m) = (an.columns(s).len(), an.super_rows[s].len());
        let update = |c: usize| (shape.update)(an.super_rows[c].len() - an.columns(c).len()) as u64;
        let incoming: u64 = tree.children[s].iter().map(|&c| update(c)).sum();
        let (front, own, kept) = ((m * m) as u64, update(s), (shape.kept)(k, m) as u64);
        let rest = held.saturating_sub(incoming);
        peak = peak.max(held.saturating_add(front)).max(rest.saturating_add(front + own + kept));
        held = rest.saturating_add(own);
    }
    (peak, held)
}

thread_local! {
    /// Whether this thread is factoring a subtree of its own.
    static IN_SUBTREE: Cell<bool> = const { Cell::new(false) };
}

/// Marks the thread as factoring a subtree of its own while it lives.
pub(super) struct SubtreeGuard(bool);

impl SubtreeGuard {
    pub fn enter() -> SubtreeGuard {
        SubtreeGuard(IN_SUBTREE.replace(true))
    }
}

impl Drop for SubtreeGuard {
    fn drop(&mut self) {
        IN_SUBTREE.set(self.0);
    }
}

/// How a front's dense product of `work` entries runs: threaded from `threshold`, but never
/// inside a subtree factored on a thread of its own. (A thread waiting on its threaded product
/// takes other jobs meanwhile — another whole subtree, its fronts held beside its own — so the
/// subtrees alive at once would no longer be bounded by the threads, nor their memory by the
/// [`Plan`]. The subtrees are threaded among themselves already.)
pub(crate) fn products(work: usize, threshold: usize) -> Par {
    if work < threshold || IN_SUBTREE.get() { Par::Seq } else { Par::rayon(0) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multifrontal::testing::grid_pattern;

    const LDLT: Shape = Shape { update: |mu| mu * (mu + 1) / 2, kept: |k, m| k * m, out_of_core: true };
    const LU: Shape = Shape { update: |mu| mu * mu, kept: |k, m| k * m + k * (m - k), out_of_core: false };

    fn analysis() -> Analysis {
        let (a, _, xyz) = grid_pattern(12, 10, 9);
        Analysis::new(&a, &xyz, 0).unwrap()
    }

    #[test]
    fn the_working_memory_bounds_the_largest_front_and_grows_with_the_threads() {
        let an = analysis();
        let tree = Tree::new(&an);
        let largest = (0..an.supernodes()).map(|s| an.super_rows[s].len().pow(2) as u64).max().unwrap();
        let one = working(&an, &tree, LDLT, 1);
        // Every front is alive once, its largest among them.
        assert!(one >= largest, "{one} < {largest}");
        assert!(working(&an, &tree, LDLT, 8) >= one);
        // LU's updates are full, so its fronts hold more.
        assert!(working(&an, &tree, LU, 1) > one);
    }

    #[test]
    fn plans_fewer_threads_then_refuse_as_the_memory_shrinks() {
        let an = analysis();
        let tree = Tree::new(&an);
        let (one, eight) = (16 * working(&an, &tree, LDLT, 1), 16 * working(&an, &tree, LDLT, 8));
        assert!(eight > one);
        let room = |bytes: u64| Some((bytes as f64 / HEADROOM).ceil() as u64 + 16);
        // Room for eight threads' fronts: all eight, the rest for the factor up to its budget.
        let p = Plan::new(&an, LDLT, u64::MAX, room(eight + 1000), 8).unwrap();
        assert_eq!(p.threads, 8);
        assert!(p.in_memory >= 1000 && p.working == eight);
        assert_eq!(Plan::new(&an, LDLT, 500, room(eight + 1000), 8).unwrap().in_memory, 500);
        // Room for one thread's only: fewer threads, the factor on disk.
        let p = Plan::new(&an, LDLT, u64::MAX, room(one), 8).unwrap();
        assert!(p.threads < 8 && p.working <= one + 16, "{p:?}");
        // Less: refused, saying what it needs.
        assert_eq!(
            Plan::new(&an, LDLT, u64::MAX, Some(one / 2), 8),
            Err(FactorError::OutOfMemory { needs: one, available: ((one / 2) as f64 * HEADROOM) as u64 })
        );
        // A factor kept in memory must fit beside its fronts.
        let lu = 16 * working(&an, &tree, LU, 1);
        let factor = 16 * factor_entries(&an, LU);
        assert!(matches!(Plan::new(&an, LU, u64::MAX, room(lu), 1), Err(FactorError::OutOfMemory { .. })));
        assert_eq!(Plan::new(&an, LU, u64::MAX, room(lu + factor), 1).unwrap().threads, 1);
        // Not known: every thread, the factor's memory capped.
        assert_eq!(Plan::new(&an, LDLT, u64::MAX, None, 8).unwrap().in_memory, UNKNOWN_BUDGET);
    }
}
