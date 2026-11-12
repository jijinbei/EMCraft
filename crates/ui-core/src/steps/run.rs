//! The steps as the window shows them and what Run checks first: the session's statuses with
//! what the window knows on top ([`shown`]), and the preflight ([`preflight`]): what blocks a
//! run and what it may accept.

use super::{Check, Solver, Status, Step, StepState, check_text};
use crate::lang::{args, plural, shown as said, tr};

/// What the window knows besides the session: the Setup form differs from the confirmed
/// settings (while some are), Run was refused for blocking problems, the last run failed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Window {
    pub unconfirmed: bool,
    pub refused: bool,
    pub failed: bool,
}

/// Whether a run of `solver` cannot go without step `step` done (the kinds' table).
pub fn required(step: Step, solver: Solver) -> bool {
    emcraft_engine::workflow::required(Some(solver)).contains(&step)
}

/// The steps' statuses `states` (the session's) as the window shows them: settings changed but
/// not confirmed make Setup to do again and Run wait; after a refused Run the steps it cannot go
/// without, left to do, need attention; after a failed run, Run does.
pub fn shown(states: [StepState; 8], w: Window, solver: Solver) -> [StepState; 8] {
    // In this order: what the form changed, what a refusal marks, what a failure marks.
    let unconfirmed = |s: StepState| match s.step {
        Step::Setup => {
            let mut messages = s.messages;
            messages.push(Check::new(t!("the settings changed: confirm them again").en(), &[], Some("project.setup")));
            StepState { status: s.status.min(Status::Todo), messages, ..s }
        }
        Step::Run if s.status > Status::Waiting => StepState {
            status: Status::Waiting,
            messages: vec![Check::new(t!("waits for {steps}").en(), &[("steps", Step::Setup.id().to_owned())], None)],
            ..s
        },
        _ => s,
    };
    let refused = |s: StepState| if required(s.step, solver) && s.status == Status::Todo { StepState { status: Status::Missing, ..s } } else { s };
    let failed = |s: StepState| if s.step == Step::Run { StepState { status: s.status.min(Status::Missing), ..s } } else { s };
    states.map(|s| {
        let s = if w.unconfirmed { unconfirmed(s) } else { s };
        let s = if w.refused { refused(s) } else { s };
        if w.failed { failed(s) } else { s }
    })
}

/// The preflight: what blocks the run and what it may accept, each with its step.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preflight {
    pub blocking: Vec<(Step, String)>,
    pub warnings: Vec<(Step, String)>,
}

/// What Run checks first: a step that needs attention, or one `solver` cannot go without left
/// to do, blocks it; warnings and out-of-date results are accepted by pressing Run; other steps
/// left to do and steps waiting on another are not problems.
pub fn preflight(solver: Solver, states: &[StepState]) -> Preflight {
    states.iter().filter(|s| s.step.index() < Step::Run.index()).fold(Preflight::default(), |mut out, s| {
        let list = match s.status {
            Status::Missing => &mut out.blocking,
            Status::Todo if required(s.step, solver) => &mut out.blocking,
            Status::Warning | Status::Stale => &mut out.warnings,
            Status::Todo | Status::Waiting | Status::Done => return out,
        };
        if s.messages.is_empty() {
            list.push((s.step, format!("{}: {}", said(s.step), said(s.status))));
        }
        list.extend(s.messages.iter().map(|m| (s.step, check_text(m))));
        out
    })
}

impl Preflight {
    /// Run's button: plain, accepting the warnings, or with the problems to fix first.
    pub fn button(&self) -> String {
        match (self.blocking.len(), self.warnings.len()) {
            (0, 0) => tr(t!("Run")).to_owned(),
            (0, n) => plural(n, t!("Run, accepting {n} warning"), t!("Run, accepting {n} warnings"), args!(n = n)),
            (n, _) => plural(n, t!("Run — {n} problem to fix first"), t!("Run — {n} problems to fix first"), args!(n = n)),
        }
    }

    /// Why Run was refused (none when nothing blocks it).
    pub fn refusal(&self) -> Option<String> {
        let n = self.blocking.len();
        (n > 0).then(|| {
            let list = self.blocking.iter().map(|(s, m)| format!("{m} ({})", s.label())).collect::<Vec<_>>().join("; ");
            format!("{n} {} {} it — {list}", if n == 1 { "problem" } else { "problems" }, if n == 1 { "blocks" } else { "block" })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(step: Step, status: Status) -> StepState {
        StepState { step, status, messages: Vec::new() }
    }

    /// A step left to do that the kind needs blocks Run; a warning is accepted; a refusal marks
    /// what blocks; settings not confirmed make Run wait.
    #[test]
    fn the_preflight_and_the_window_marks() {
        let states = Step::ALL.map(|s| {
            state(
                s,
                if s == Step::Mesh {
                    Status::Todo
                } else if s == Step::Boundaries {
                    Status::Warning
                } else {
                    Status::Done
                },
            )
        });
        let p = preflight(Solver::Frequency, &states);
        assert_eq!((p.blocking.len(), p.warnings.len()), (1, 1));
        assert!(p.refusal().is_some_and(|r| r.starts_with("1 problem blocks it")));
        let refused = shown(states.clone(), Window { refused: true, ..Window::default() }, Solver::Frequency);
        assert_eq!(refused[Step::Mesh.index()].status, Status::Missing);
        let waiting = shown(states, Window { unconfirmed: true, ..Window::default() }, Solver::Frequency);
        assert_eq!((waiting[Step::Setup.index()].status, waiting[Step::Run.index()].status), (Status::Done.min(Status::Todo), Status::Waiting));
    }
}
