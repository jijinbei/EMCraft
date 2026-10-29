//! Window screenshots asked for over the control channel (`ui.screenshot`): each waits for a
//! presented frame, and gives up after a while when none comes (a hidden window).

use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::time::Instant;

use serde_json::{Value, json};

use crate::{EmcraftApp, control};

/// How long a screenshot waits for a frame (s).
const PATIENCE: f64 = 8.0;

/// A window screenshot waiting for its frame.
pub(crate) struct Shot {
    pub token: u64,
    pub path: Option<String>,
    pub reply: Sender<Value>,
    pub asked: Instant,
    pub sent: bool,
}

impl EmcraftApp {
    /// Asks the window for a screenshot (answered in a later frame).
    pub(crate) fn request_screenshot(&mut self, path: Option<String>, reply: Sender<Value>) {
        self.control.shot_token += 1;
        self.control.shots.push(Shot { token: self.control.shot_token, path, reply, asked: Instant::now(), sent: false });
    }

    /// Asks for the frames of new screenshots, answers those whose frame came, and those that
    /// waited too long.
    pub(crate) fn collect_screenshots(&mut self, ctx: &egui::Context) {
        if self.control.shots.is_empty() {
            return;
        }
        for s in self.control.shots.iter_mut().filter(|s| !s.sent) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(s.token)));
            s.sent = true;
        }
        let events: Vec<(u64, Arc<egui::ColorImage>)> = ctx.input(|i| {
            i.raw
                .events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Screenshot { user_data, image, .. } => {
                        let token = user_data.data.as_ref().and_then(|d| d.downcast_ref::<u64>()).copied()?;
                        Some((token, image.clone()))
                    }
                    _ => None,
                })
                .collect()
        });
        for (token, image) in events {
            if let Some(i) = self.control.shots.iter().position(|s| s.token == token) {
                let shot = self.control.shots.remove(i);
                let _ = shot.reply.send(control::save_screenshot(&image, shot.path.as_deref()));
            }
        }
        self.control.shots.retain(|s| {
            if s.asked.elapsed().as_secs_f64() < PATIENCE {
                return true;
            }
            let _ = s.reply.send(json!({"ok": false, "error": "no frame was presented (window hidden?); use ui.render"}));
            false
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}
