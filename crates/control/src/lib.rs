//! EMCraft's control channel, whatever window serves it (docs/control-protocol.md): a request
//! (`{method, params}`) with where its reply goes ([`ControlRequest`]), and the loopback
//! JSON-lines server that queues them for the window, behind a per-launch token ([`server`],
//! [`auth`]). Each window handles the requests between its frames its own way.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod auth;
pub mod server;

use std::sync::mpsc::Sender;

use serde_json::Value;

/// A request from the control channel, with where its reply goes.
pub struct ControlRequest {
    pub method: String,
    pub params: Value,
    pub reply: Sender<Value>,
}

impl ControlRequest {
    pub fn new(method: impl Into<String>, params: Value) -> (ControlRequest, std::sync::mpsc::Receiver<Value>) {
        let (tx, rx) = std::sync::mpsc::channel();
        (ControlRequest { method: method.into(), params, reply: tx }, rx)
    }
}

impl std::fmt::Debug for ControlRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ControlRequest").field("method", &self.method).field("params", &self.params).finish()
    }
}
