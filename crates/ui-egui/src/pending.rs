//! Work drawn off the frame: a picture that takes long to make (a template's model built and
//! drawn) is made on a thread of its own while the window keeps answering, the frame showing
//! what it had until it is done. At most [`MAX_RUNNING`] run at once; the rest wait their turn.
//!
//! The headless scenarios make it at once instead (`inline`), so what they read after a step is
//! what that step made, the same every run.

use std::cell::Cell;
use std::sync::{Arc, Condvar, Mutex};

/// Pictures made at once, across the window.
const MAX_RUNNING: usize = 2;

/// Jobs running now, and the signal a waiting one wakes on.
static RUNNING: (Mutex<usize>, Condvar) = (Mutex::new(0), Condvar::new());

thread_local! {
    /// Whether this thread makes its pending work at once (the scenarios' harness).
    static INLINE: Cell<bool> = const { Cell::new(false) };
}

/// Makes this thread's pending work at once from now on (for the headless scenarios).
pub fn inline_here() {
    INLINE.set(true);
}

/// One piece of work under way, or done and not yet taken.
pub struct Pending<T> {
    slot: Arc<Mutex<Option<T>>>,
}

impl<T: Send + 'static> Pending<T> {
    /// Starts `work` on its own thread (at once on an inline thread); `ctx` is asked to draw
    /// again when it is done.
    pub fn start(ctx: &egui::Context, work: impl FnOnce() -> T + Send + 'static) -> Pending<T> {
        let slot = Arc::new(Mutex::new(None));
        if INLINE.get() || cfg!(test) {
            put(&slot, work());
            return Pending { slot };
        }
        let (into, ctx) = (Arc::clone(&slot), ctx.clone());
        std::thread::spawn(move || {
            let _turn = Turn::wait();
            put(&into, work());
            ctx.request_repaint();
        });
        Pending { slot }
    }

    /// The work's result once it is done, once.
    pub fn take(&self) -> Option<T> {
        self.slot.lock().ok().and_then(|mut s| s.take())
    }
}

/// Puts a result in its slot (a poisoned slot keeps nothing: the work is lost, never a panic).
fn put<T>(slot: &Mutex<Option<T>>, value: T) {
    if let Ok(mut s) = slot.lock() {
        *s = Some(value);
    }
}

/// A place among the [`MAX_RUNNING`] jobs, given back when dropped.
struct Turn;

impl Turn {
    fn wait() -> Turn {
        let (count, wake) = &RUNNING;
        if let Ok(mut n) = count.lock() {
            while *n >= MAX_RUNNING {
                match wake.wait(n) {
                    Ok(g) => n = g,
                    Err(_) => return Turn,
                }
            }
            *n += 1;
        }
        Turn
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        let (count, wake) = &RUNNING;
        if let Ok(mut n) = count.lock() {
            *n = n.saturating_sub(1);
        }
        wake.notify_one();
    }
}
