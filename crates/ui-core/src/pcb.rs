//! A board's diagnosis as the windows show it: the route chosen (while it is one of the board's),
//! the report kept for it (while it was made under the conditions confirmed) and the findings'
//! names.

use emcraft_engine::Session;
use emcraft_engine::pcb::{Code, Inspection};

use crate::lang::Text;
use crate::replies::Replies;

/// The route chosen, while it is one of the board's.
pub fn route(s: &Session) -> Option<&str> {
    s.views().pcb.net().filter(|n| s.model().pcb().is_some_and(|b| b.segments.iter().any(|g| g.net == *n)))
}

/// The report of `net` kept, while it was made under the confirmed conditions.
pub fn report<'a>(s: &Session, replies: &'a Replies, net: &str) -> Option<&'a Inspection> {
    let c = s.views().pcb.conditions()?;
    replies.pcb.as_ref().filter(|r| {
        r.net == net
            && r.ground == c.ground
            && r.reference == c.reference
            && r.stop_hz == c.stop_hz
            && r.frequency_unit == s.display().frequency.symbol()
    })
}

/// A finding's name.
pub fn finding_label(code: Code) -> Text {
    match code {
        Code::ReturnGap => t!("Reference copper missing beneath the route"),
        Code::ReferenceEdge => t!("Return corridor reaches a reference edge or slot"),
        Code::NoReference => t!("No filled reference plane"),
        Code::RouteTopology => t!("Route is branched, disconnected or closed"),
        Code::ViaTransition => t!("Signal via needs a transition model"),
        Code::MissingReturnVia => t!("Verify the return via at this layer transition"),
        Code::NearbySignal => t!("Nearby parallel signal: check coupling"),
        Code::UnsupportedCrossSection => t!("Cross-section outside the supported line model"),
    }
}
