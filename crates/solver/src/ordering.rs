//! Fill-reducing ordering: nested dissection by coordinate bisection.
//!
//! Nested dissection (A. George, *Nested dissection of a regular finite element mesh*, SIAM J.
//! Numer. Anal. 10(2):345–363, 1973) orders a separator after the two parts it splits, so the
//! factor of a 3D mesh with n unknowns fills O(n^{4/3}) instead of the O(n^{5/3}) of a band
//! ordering. The separator is geometric: the unknowns are split by a plane across one axis of
//! their bounding box, and those of the smaller side that touch the other side form the
//! separator (the coordinate nested dissection of H. D. Simon, *Partitioning of unstructured
//! problems for parallel processing*, Computing Systems in Engineering 2:135–148, 1991). Planes
//! across each axis at the median and at cuts up to 30/70 are tried and the smallest separator
//! kept: the separator's size is what the factorization's largest fronts, cubic in it, pay for
//! (the median of the longest axis alone cut the magic tee through its three arms and its posts'
//! fine mesh: a root front of 11 168 rows and 1.0 Tflop, against 3 958 rows and 0.19 Tflop).
//! Ties are broken by index, so the ordering is deterministic. A waveguide is long and thin:
//! its separators are cross-sections, and the fronts stay small.
//!
//! Adapted from FieldCraft's `crates/solver/src/ordering.rs` (MIT OR Apache-2.0), see
//! ATTRIBUTION.md.

use emcraft_model::Point3;

/// Parts this small are ordered as they come.
const LEAF: usize = 16;

/// Where a part may be cut along an axis, as fractions of its unknowns in that axis' order: the
/// median, then ever further from it, to at most 30/70.
const CUTS: &[f64] = &[0.5, 0.46, 0.54, 0.42, 0.58, 0.38, 0.62, 0.34, 0.66, 0.3, 0.7];

/// A permutation of the nodes: `order[k]` is the node eliminated k-th.
pub fn nested_dissection(xyz: &[Point3], adjacency: &[Vec<usize>]) -> Vec<usize> {
    dissection(xyz, adjacency, CUTS, 3)
}

/// Nested dissection by planes at `cuts` across the `axes` longest axes of each part.
fn dissection(xyz: &[Point3], adjacency: &[Vec<usize>], cuts: &[f64], axes: usize) -> Vec<usize> {
    let most_axes = axes;
    let n = xyz.len();
    let mut order = Vec::with_capacity(n);
    // 0 = not in the current part, 1 = left, 2 = right.
    let mut side = vec![0u8; n];
    // An explicit stack instead of recursion: (part, separator to emit after it).
    enum Task {
        Split(Vec<usize>),
        Emit(Vec<usize>),
    }
    let mut stack = vec![Task::Split((0..n).collect())];
    while let Some(task) = stack.pop() {
        let mut part = match task {
            Task::Emit(sep) => {
                order.extend(sep);
                continue;
            }
            Task::Split(p) => p,
        };
        if part.len() <= LEAF {
            order.extend(part);
            continue;
        }
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for &v in &part {
            let p = xyz.get(v).copied().unwrap_or_default();
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        // Each axis with an extent, the longest first, cut at each of `CUTS` (fractions of the
        // part, the median first): the smaller of a cut's two sides' boundaries is its separator,
        // and the smallest separator wins (the earlier cut on a tie). A junction's middle plane
        // can cut its arms lengthwise where another axis, or a cut beside a post's fine mesh,
        // crosses one guide.
        let mut axes: Vec<usize> = (0..3).filter(|&k| hi[k] > lo[k]).collect();
        axes.sort_by(|&a, &b| (hi[b] - lo[b]).total_cmp(&(hi[a] - lo[a])).then(a.cmp(&b)));
        axes.truncate(most_axes);
        if axes.is_empty() {
            axes.push(0);
        }
        let mut sorted_by: Vec<Vec<usize>> = Vec::with_capacity(axes.len());
        // (index into `sorted_by`, cut, separator)
        let mut best: Option<(usize, usize, Vec<usize>)> = None;
        for &axis in &axes {
            let key = |v: usize| xyz.get(v).map_or(0.0, |p| p[axis]);
            let mut sorted = part.clone();
            sorted.sort_by(|&a, &b| key(a).total_cmp(&key(b)).then(a.cmp(&b)));
            for &fraction in cuts {
                let cut = ((part.len() as f64 * fraction) as usize).clamp(1, part.len() - 1);
                for (i, &v) in sorted.iter().enumerate() {
                    if let Some(s) = side.get_mut(v) {
                        *s = if i < cut { 1 } else { 2 };
                    }
                }
                let touches = |v: usize, other: u8| adjacency.get(v).is_some_and(|row| row.iter().any(|w| side.get(*w) == Some(&other)));
                let (left, right) = sorted.split_at(cut);
                let left_boundary: Vec<usize> = left.iter().copied().filter(|&v| touches(v, 2)).collect();
                let right_boundary: Vec<usize> = right.iter().copied().filter(|&v| touches(v, 1)).collect();
                let separator = if left_boundary.len() <= right_boundary.len() { left_boundary } else { right_boundary };
                if best.as_ref().is_none_or(|(_, _, b)| separator.len() < b.len()) {
                    best = Some((sorted_by.len(), cut, separator));
                }
            }
            sorted_by.push(sorted);
        }
        let Some((k, half, separator)) = best else {
            order.extend(part);
            continue;
        };
        part = sorted_by.get_mut(k).map(std::mem::take).unwrap_or(part);
        let (left, right) = part.split_at(half);
        for &v in &part {
            if let Some(s) = side.get_mut(v) {
                *s = 0;
            }
        }
        // Mark the separator so the parts leave it out.
        for &v in &separator {
            if let Some(s) = side.get_mut(v) {
                *s = 3;
            }
        }
        let a: Vec<usize> = left.iter().copied().filter(|v| side.get(*v) != Some(&3)).collect();
        let b: Vec<usize> = right.iter().copied().filter(|v| side.get(*v) != Some(&3)).collect();
        for &v in &separator {
            if let Some(s) = side.get_mut(v) {
                *s = 0;
            }
        }
        // Order: a, then b, then the separator (the stack runs in reverse).
        stack.push(Task::Emit(separator));
        stack.push(Task::Split(b));
        stack.push(Task::Split(a));
    }
    order
}

/// The nested dissection order of the nodes that fills the factor less: by coordinate planes or
/// by the graph's level structures. Ties keep the coordinate order.
pub fn fill_reducing_order(xyz: &[Point3], adjacency: &[Vec<usize>]) -> Vec<usize> {
    let by_planes = nested_dissection(xyz, adjacency);
    if adjacency.len() <= LEAF {
        // Both order a leaf as it comes: there is no choice to make.
        return by_planes;
    }
    let by_graph = graph_dissection(adjacency);
    if fill(adjacency, &by_graph) < fill(adjacency, &by_planes) { by_graph } else { by_planes }
}

/// [`fill_reducing_order`] of the first n − `last` nodes, the last `last` after them all: the
/// unknowns that border a matrix (one coupled to many, a port mode's) are eliminated at the end,
/// where they fill only their own rows.
pub fn fill_reducing_order_last(xyz: &[Point3], adjacency: &[Vec<usize>], last: usize) -> Vec<usize> {
    let n = adjacency.len();
    let k = n.saturating_sub(last);
    if k == n {
        return fill_reducing_order(xyz, adjacency);
    }
    let inner: Vec<Vec<usize>> = adjacency.iter().take(k).map(|row| row.iter().copied().filter(|&j| j < k).collect()).collect();
    let mut order = fill_reducing_order(xyz.get(..k).unwrap_or(xyz), &inner);
    order.extend(k..n);
    order
}

/// Searches for a pseudo-peripheral node per part.
const PERIPHERAL_SEARCHES: usize = 8;

/// The breadth-first level structure of the nodes with `owner == token` reached from `root`.
/// Marks each reached node's level in `level` (which must be `usize::MAX` there on entry).
fn level_structure(adjacency: &[Vec<usize>], root: usize, token: usize, owner: &[usize], level: &mut [usize]) -> Vec<Vec<usize>> {
    let mut out = vec![vec![root]];
    if let Some(l) = level.get_mut(root) {
        *l = 0;
    }
    loop {
        let depth = out.len();
        let mut next = Vec::new();
        for &v in out.last().into_iter().flatten() {
            for &w in adjacency.get(v).into_iter().flatten() {
                if owner.get(w) == Some(&token)
                    && let Some(l) = level.get_mut(w)
                    && *l == usize::MAX
                {
                    *l = depth;
                    next.push(w);
                }
            }
        }
        if next.is_empty() {
            return out;
        }
        out.push(next);
    }
}

/// Clears the levels of `structure`'s nodes.
fn clear(level: &mut [usize], structure: &[Vec<usize>]) {
    for &v in structure.iter().flatten() {
        if let Some(l) = level.get_mut(v) {
            *l = usize::MAX;
        }
    }
}

/// A permutation of the nodes by nested dissection on the graph alone (George & Liu 1978, see
/// the module notes): each part's separator is the nodes of its middle level that touch the
/// next level. Disconnected parts are ordered one after the other. Ties go by node index, so
/// the ordering is deterministic.
pub fn graph_dissection(adjacency: &[Vec<usize>]) -> Vec<usize> {
    let n = adjacency.len();
    let mut order = Vec::with_capacity(n);
    // The part each node is in (a token per part), and its level in the current search.
    let mut owner = vec![0usize; n];
    let mut level = vec![usize::MAX; n];
    let mut token = 0usize;
    enum Task {
        Split(Vec<usize>),
        Emit(Vec<usize>),
    }
    let mut stack = vec![Task::Split((0..n).collect())];
    while let Some(task) = stack.pop() {
        let part = match task {
            Task::Emit(sep) => {
                order.extend(sep);
                continue;
            }
            Task::Split(p) => p,
        };
        let Some(&first) = part.first() else { continue };
        if part.len() <= LEAF {
            order.extend(part);
            continue;
        }
        token += 1;
        for &v in &part {
            if let Some(o) = owner.get_mut(v) {
                *o = token;
            }
        }
        let degree = |v: usize, owner: &[usize]| adjacency.get(v).map_or(0, |row| row.iter().filter(|w| owner.get(**w) == Some(&token)).count());
        // A pseudo-peripheral root: from the first node, move to the least connected node of the
        // last level while that deepens the structure.
        let mut structure = level_structure(adjacency, first, token, &owner, &mut level);
        for _ in 0..PERIPHERAL_SEARCHES {
            let Some(candidate) = structure.last().and_then(|last| last.iter().copied().min_by_key(|&v| (degree(v, &owner), v))) else { break };
            clear(&mut level, &structure);
            let deeper = level_structure(adjacency, candidate, token, &owner, &mut level);
            if deeper.len() <= structure.len() {
                // Keep the structure found: restore its levels.
                clear(&mut level, &deeper);
                for (depth, l) in structure.iter().enumerate() {
                    for &v in l {
                        if let Some(x) = level.get_mut(v) {
                            *x = depth;
                        }
                    }
                }
                break;
            }
            structure = deeper;
        }
        let reached: usize = structure.iter().map(Vec::len).sum();
        if reached < part.len() {
            // Disconnected: the reached component, then the rest.
            let rest: Vec<usize> = part.iter().copied().filter(|&v| level.get(v) == Some(&usize::MAX)).collect();
            clear(&mut level, &structure);
            let mut component: Vec<usize> = structure.into_iter().flatten().collect();
            component.sort_unstable();
            stack.push(Task::Split(rest));
            stack.push(Task::Split(component));
            continue;
        }
        if structure.len() < 3 {
            clear(&mut level, &structure);
            order.extend(part);
            continue;
        }
        // The middle level: the first at which the levels up to it hold half the part, kept
        // off the last level so both sides are non-empty.
        let mut count = 0;
        let mut middle = 1;
        for (i, l) in structure.iter().enumerate().skip(1) {
            count += l.len();
            if 2 * count >= part.len() {
                middle = i.min(structure.len() - 2);
                break;
            }
        }
        let touches_next =
            |v: usize| adjacency.get(v).is_some_and(|row| row.iter().any(|&w| owner.get(w) == Some(&token) && level.get(w) == Some(&(middle + 1))));
        let mut separator = Vec::new();
        let mut before: Vec<usize> = structure.iter().take(middle).flatten().copied().collect();
        for &v in structure.get(middle).into_iter().flatten() {
            if touches_next(v) { separator.push(v) } else { before.push(v) }
        }
        let mut after: Vec<usize> = structure.iter().skip(middle + 1).flatten().copied().collect();
        clear(&mut level, &structure);
        before.sort_unstable();
        separator.sort_unstable();
        after.sort_unstable();
        // Order: before, then after, then the separator (the stack runs in reverse).
        stack.push(Task::Emit(separator));
        stack.push(Task::Split(after));
        stack.push(Task::Split(before));
    }
    order
}

/// One step of Liu's elimination-tree algorithm with path compression: row k's entry in
/// column i < k links the root of i's subtree to k (`parent`), every node on the way pointed at
/// k (`ancestor`; `usize::MAX` for none yet).
pub(crate) fn etree_link(parent: &mut [usize], ancestor: &mut [usize], i: usize, k: usize) {
    let mut r = i;
    while ancestor[r] != usize::MAX && ancestor[r] != k {
        let next = ancestor[r];
        ancestor[r] = k;
        r = next;
    }
    if ancestor[r] == usize::MAX {
        ancestor[r] = k;
        parent[r] = k;
    }
}

/// The entries of the Cholesky factor (lower triangle with the diagonal) of the graph
/// `adjacency` eliminated in `order`: for each row k, the row's structure is the union of the
/// elimination-tree paths from its earlier neighbours up to k (J. W. H. Liu, *A compact row
/// storage scheme for Cholesky factors using elimination trees*, ACM TOMS 12(2):127–148, 1986),
/// counted with a marker per node: O(|L|) time, O(n) memory.
pub fn fill(adjacency: &[Vec<usize>], order: &[usize]) -> usize {
    let n = adjacency.len();
    let mut position = vec![usize::MAX; n];
    for (k, &v) in order.iter().enumerate() {
        if let Some(p) = position.get_mut(v) {
            *p = k;
        }
    }
    let mut parent = vec![usize::MAX; n];
    let mut ancestor = vec![usize::MAX; n];
    let mut mark = vec![usize::MAX; n];
    let mut total = 0usize;
    for (k, &v) in order.iter().enumerate() {
        mark[k] = k;
        total += 1;
        for &w in adjacency.get(v).into_iter().flatten() {
            let Some(&i) = position.get(w) else { continue };
            if i >= k {
                continue;
            }
            etree_link(&mut parent, &mut ancestor, i, k);
            // Row k's structure: from i up the tree until a node already counted for k.
            let mut j = i;
            while j != usize::MAX && j < k && mark[j] != k {
                mark[j] = k;
                total += 1;
                j = parent[j];
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_permutation_with_separators_last() {
        let m = 12;
        let xyz: Vec<Point3> = (0..m * m).map(|v| Point3::new((v / m) as f64, (v % m) as f64, 0.0)).collect();
        let mut adj = vec![Vec::new(); m * m];
        for i in 0..m {
            for j in 0..m {
                for (di, dj) in [(0i64, 1i64), (1, 0), (0, -1), (-1, 0)] {
                    let (a, b) = (i as i64 + di, j as i64 + dj);
                    if (0..m as i64).contains(&a) && (0..m as i64).contains(&b) {
                        adj[i * m + j].push(a as usize * m + b as usize);
                    }
                }
            }
        }
        let order = nested_dissection(&xyz, &adj);
        let mut sorted = order.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..m * m).collect::<Vec<_>>());
        // The first separator (eliminated last) is a grid line: it splits the grid in two.
        let last: Vec<usize> = order[order.len() - m..].to_vec();
        let rows: std::collections::BTreeSet<usize> = last.iter().map(|v| v / m).collect();
        assert_eq!(rows.len(), 1, "{last:?}");
        assert_eq!(order, nested_dissection(&xyz, &adj));
        // The graph's dissection is a permutation too, and the better of the two fills no more.
        let g = graph_dissection(&adj);
        let mut sorted = g.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..m * m).collect::<Vec<_>>());
        let best = fill_reducing_order(&xyz, &adj);
        assert!(fill(&adj, &best) <= fill(&adj, &order).min(fill(&adj, &g)));
    }

    #[test]
    fn fill_of_a_chain_and_of_a_star() {
        // A path in its natural order: no fill, 2n − 1 entries.
        let n: usize = 10;
        let adj: Vec<Vec<usize>> = (0..n).map(|i| [i.wrapping_sub(1), i + 1].into_iter().filter(|&j| j < n).collect()).collect();
        assert_eq!(fill(&adj, &(0..n).collect::<Vec<_>>()), 2 * n - 1);
        // A star with its centre first fills completely: n(n+1)/2.
        let star: Vec<Vec<usize>> = (0..n).map(|i| if i == 0 { (1..n).collect() } else { vec![0] }).collect();
        assert_eq!(fill(&star, &(0..n).collect::<Vec<_>>()), n * (n + 1) / 2);
        // Centre last: no fill.
        let mut last: Vec<usize> = (1..n).collect();
        last.push(0);
        assert_eq!(fill(&star, &last), 2 * n - 1);
    }

    #[test]
    fn a_junction_is_cut_across_one_arm() {
        // A T of grid nodes: a bar along x (40 × 6 × 4) and a stem along y from its middle
        // (6 × 24 × 4). The median across the longest axis (x) cuts the stem lengthwise, a
        // separator of 120 nodes, where a plane across one arm takes 24: trying every axis and
        // cuts beside the median fills the factor far less.
        let mut xyz: Vec<Point3> = Vec::new();
        let mut index = std::collections::BTreeMap::new();
        for x in 0..40i64 {
            for y in 0..30i64 {
                for z in 0..4i64 {
                    if y < 6 || (17..23).contains(&x) {
                        index.insert((x, y, z), xyz.len());
                        xyz.push(Point3::new(x as f64, y as f64, z as f64));
                    }
                }
            }
        }
        let mut adj = vec![Vec::new(); xyz.len()];
        for (&(x, y, z), &v) in &index {
            for d in [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)] {
                if let Some(&w) = index.get(&(x + d.0, y + d.1, z + d.2)) {
                    adj[v].push(w);
                }
            }
        }
        let best = fill(&adj, &nested_dissection(&xyz, &adj));
        let longest = fill(&adj, &dissection(&xyz, &adj, &[0.5], 1));
        // Measured: 47 436 entries against 73 494.
        assert!(10 * best < 8 * longest, "{best} {longest}");
    }
}
