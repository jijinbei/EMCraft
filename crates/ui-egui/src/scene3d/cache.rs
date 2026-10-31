//! What a built scene keeps beside it, made once per scene: its picking index
//! ([`emcraft_render::view3d::SceneIndex`]) and, for a large scene, a coarser copy drawn while
//! the view moves ([`emcraft_render::view3d::simplify::coarser`], Garland–Heckbert quadric
//! simplification).
//!
//! A small scene's index is made when first asked for. A large one's, and its coarser copy, are
//! made on a thread of their own; until they are, picks scan every triangle (the same answer,
//! [`SceneIndex::hits`] is the scan's tie for tie) and the view moves at full detail. Picking,
//! snapping and measuring always read the full scene; only a moving picture is drawn coarser.
//! A clone starts empty: the copy's scene may change.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use emcraft_render::view3d::{Scene, SceneIndex, simplify};

/// From this many triangles the index is made on a thread (a small scene's takes under a few
/// milliseconds).
const BACKGROUND_FROM: usize = 20_000;
/// From this many triangles a scene gets a coarser copy for moving pictures…
pub(crate) const COARSE_FROM: usize = 80_000;
/// …of about this many triangles.
pub(crate) const COARSE_TRIANGLES: usize = 40_000;
/// Scenes worked on at once (more wait for a later frame).
const MAX_JOBS: usize = 2;
/// Jobs running now, across every scene.
static RUNNING: AtomicUsize = AtomicUsize::new(0);

/// The index and coarser copy of one scene.
#[derive(Default)]
pub struct Cache(OnceLock<Arc<Made>>);

#[derive(Default)]
struct Made {
    index: OnceLock<SceneIndex>,
    /// `None` inside: the scene needs no coarser copy, or none could be made.
    coarse: OnceLock<Option<Scene>>,
    started: AtomicBool,
}

impl Made {
    /// Makes what `scene` needs: its index, then its coarser copy. Stops early when nobody holds
    /// it any more (`alone`).
    fn fill(&self, scene: &Scene, alone: impl Fn() -> bool) {
        self.index.get_or_init(|| SceneIndex::new(scene));
        if alone() {
            return;
        }
        self.coarse.get_or_init(|| coarse(scene));
    }
}

/// `scene`'s coarser copy, when it is large enough to need one.
fn coarse(scene: &Scene) -> Option<Scene> {
    (scene.tris.len() >= COARSE_FROM).then(|| simplify::coarser(scene, COARSE_TRIANGLES)).flatten()
}

impl Cache {
    /// Nothing made yet.
    pub const fn none() -> Cache {
        Cache(OnceLock::new())
    }

    fn made(&self) -> &Arc<Made> {
        self.0.get_or_init(Arc::default)
    }

    /// Starts making what `scene` needs, once: at once for a small scene, on a thread for a large
    /// one (when a thread is free; else a later call starts it).
    pub fn start(&self, scene: &Scene) {
        let made = self.made();
        if made.started.load(Ordering::Acquire) {
            return;
        }
        if scene.tris.len() < BACKGROUND_FROM {
            made.started.store(true, Ordering::Release);
            made.fill(scene, || false);
            return;
        }
        if RUNNING.fetch_add(1, Ordering::AcqRel) >= MAX_JOBS {
            RUNNING.fetch_sub(1, Ordering::AcqRel);
            return;
        }
        made.started.store(true, Ordering::Release);
        let (job, copy) = (made.clone(), scene.clone());
        let spawned = std::thread::Builder::new().name("scene-index".into()).spawn(move || {
            job.fill(&copy, || Arc::strong_count(&job) == 1);
            RUNNING.fetch_sub(1, Ordering::AcqRel);
        });
        if spawned.is_err() {
            // No thread: the index at once, no coarser copy.
            RUNNING.fetch_sub(1, Ordering::AcqRel);
            made.index.get_or_init(|| SceneIndex::new(scene));
            let _ = made.coarse.set(None);
        }
    }

    /// `scene`'s index when made (starting it otherwise).
    pub fn index(&self, scene: &Scene) -> Option<&SceneIndex> {
        self.start(scene);
        self.0.get().and_then(|m| m.index.get())
    }

    /// `scene`'s coarser copy for moving pictures, when it needs one and it is made (starting it
    /// otherwise).
    pub fn coarse(&self, scene: &Scene) -> Option<&Scene> {
        self.start(scene);
        self.0.get().and_then(|m| m.coarse.get()).and_then(Option::as_ref)
    }

    /// Makes everything now, on this thread (tests).
    #[cfg(test)]
    pub fn fill_now(&self, scene: &Scene) {
        let made = self.made();
        made.started.store(true, Ordering::Release);
        made.fill(scene, || false);
    }
}

impl Clone for Cache {
    /// Empty: the copy's scene may change.
    fn clone(&self) -> Cache {
        Cache::none()
    }
}

impl std::fmt::Debug for Cache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let made = self.0.get();
        f.debug_struct("Cache")
            .field("index", &made.is_some_and(|m| m.index.get().is_some()))
            .field("coarse", &made.and_then(|m| m.coarse.get()).map(|c| c.as_ref().map(|s| s.tris.len())))
            .finish()
    }
}
