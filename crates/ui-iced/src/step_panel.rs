//! The open step's panel at the right of the page: its header (the
//! status's mark, `Step n · name`, the status), why the kind chosen does without it, its checks
//! each with the Fix it leads to, and the step's own part — the geometry's bodies and materials,
//! the boundaries, the mesh's statistics and the solve's estimate, the ports with their roles
//! and the port-mode preview, the run's settings over the band's diagram with the device, Run's
//! preflight, and the results' checks and Touchstone export. Everything it changes is a command.

use emcraft_engine::workflow::{Status, Step, StepState};
use emcraft_engine::{Device, Precision};
use emcraft_ui_core::lang::{Text, args, lookup, plural, shown, tr, trf};
use emcraft_ui_core::steps::{Solver, check_text, fix_label};
use iced::widget::{Column, Space, button, canvas, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Color, Element, Fill, Length, Widget};
use serde_json::Value;

use crate::app::{App, Message};
use crate::look::{self, Tokens};

/// What the panel's controls ask of the window.
#[derive(Clone, Debug)]
pub enum Act {
    /// A check's Fix: its command's step, and its dialog.
    Fix(String),
    /// A Setup field typed, by name.
    Typed(&'static str, String),
    Confirm,
    Run,
    Device(Device, Precision),
    PreviewModes,
    CheckResults,
    /// The Touchstone export's format (`ri`, `ma`), and the export.
    Format(&'static str),
    Export,
}

fn send(a: Act) -> Message {
    Message::StepAct(a)
}

fn small<'a>(t: Tokens, s: impl Into<String>, ink: Color) -> Element<'a, Message> {
    text(s.into()).size(t.font_small).color(ink).boxed()
}

fn section<'a>(t: Tokens, title: Text) -> Element<'a, Message> {
    text(tr(title).to_uppercase()).size(t.font_small).color(t.text_dim).boxed()
}

/// A name and its value on a line.
fn kv<'a>(t: Tokens, k: Text, v: String) -> Element<'a, Message> {
    let v = emcraft_render::math::to_unicode(&v);
    row![text(tr(k)).size(t.font_small).color(t.text_dim).width(120), text(v).size(t.font_small).color(t.text)].spacing(6).boxed()
}

fn action<'a>(t: Tokens, label: impl Into<String>, primary: bool, on: Option<Message>) -> Element<'a, Message> {
    button(text(label.into()).size(t.font_button)).style(look::button(t, primary)).on_press_maybe(on).boxed()
}

/// A choice among several: the one chosen marked ✔ in the accent, the others buttons.
fn chip<'a>(t: Tokens, label: &str, on: bool, choose: Option<Message>) -> Element<'a, Message> {
    if on {
        return container(text(format!("✔ {label}")).size(t.font_button).color(t.accent)).padding([4, 8]).boxed();
    }
    button(text(label.to_owned()).size(t.font_button)).style(look::button(t, false)).on_press_maybe(choose).boxed()
}

/// A status's colour.
fn colour(t: Tokens, s: Status) -> Color {
    crate::steps::mark(t, s).1
}

/// The open step's panel, when one is open.
pub fn view(app: &App, width: f32) -> Option<Element<'_, Message>> {
    // A result's page shows the results' panel, whichever step was open.
    let step = match app.workflow.step? {
        _ if app.page.is_result() && app.page != emcraft_ui_core::view::View::PortModes => Step::Results,
        step => step,
    };
    let t = app.look.tokens;
    let states = app.statuses();
    let state = states.get(step.index())?.clone();
    let running = app.running();
    let head = header(app, &state, running);
    let unneeded = emcraft_ui_core::steps::not_needed(app.session.model(), step).map(|why| small(t, why, t.text_dim));
    let checks = (!(step == Step::Run && running)).then(|| checks(t, &state));
    let body: Vec<Element<Message>> = match step {
        Step::Intent => Vec::new(),
        Step::Geometry => geometry(app),
        Step::Boundaries => boundaries(app),
        Step::Mesh => mesh(app),
        Step::Excitation => excitation(app),
        Step::Setup => setup(app),
        Step::Run => run(app, &states),
        Step::Results => results(app, &states),
    };
    let parts = std::iter::once(head).chain(unneeded).chain(checks).chain(body);
    let list = Column::with_children(parts).spacing(8).padding(12);
    Some(container(scrollable(list).height(Fill)).width(width).height(Fill).style(look::ground(t)).boxed())
}

/// The status's mark, `Step n · name` and the status.
fn header<'a>(app: &App, s: &StepState, running: bool) -> Element<'a, Message> {
    let t = app.look.tokens;
    let going = running && s.step == Step::Run;
    let (symbol, ink) = if going { ("◌", t.accent) } else { crate::steps::mark(t, s.status) };
    let said = if going { tr(t!("running")).to_owned() } else { shown(s.status).to_owned() };
    let title = trf(t!("Step {n} · {step}"), args!(n = s.step.index(), step = shown(s.step)));
    row![text(symbol).size(t.font_heading).color(ink), text(title).size(t.font_heading).color(t.text), text(said).size(t.font_small).color(ink)]
        .spacing(8)
        .align_y(Alignment::Center)
        .boxed()
}

/// The step's checks, each with its fix; "Nothing to fix" when it is done.
fn checks<'a>(t: Tokens, s: &StepState) -> Element<'a, Message> {
    if s.messages.is_empty() {
        return if s.status == Status::Done { small(t, tr(t!("Nothing to fix here.")), t.success) } else { Space::new().boxed() };
    }
    let ink = if s.status == Status::Done { t.text_dim } else { colour(t, s.status) };
    let rows = s.messages.iter().map(|m| {
        let fix = m.command.as_ref().and_then(|c| fix_label(c, s.step).map(|label| action(t, label, false, Some(send(Act::Fix(c.clone()))))));
        column![text(check_text(m)).size(t.font_small).color(ink)].push(fix.unwrap_or_else(|| Space::new().boxed())).spacing(4).boxed()
    });
    Column::with_children(rows).spacing(6).boxed()
}

/// The geometry: the two ways to begin while there is none (a SolidCraft design, a template), the bodies, the extent, the
/// materials and New material.
fn geometry(app: &App) -> Vec<Element<'_, Message>> {
    let t = app.look.tokens;
    let m = app.session.model();
    let idle = app.job.is_none();
    let begin = m.bounds().is_none().then(|| {
        row![
            action(t, tr(t!("Import a SolidCraft design")), true, idle.then(|| send(Act::Fix("geometry.import".into())))),
            // A shape is a design's or a template's (docs/adr/0010-geometry-from-solidcraft.md).
            action(t, tr(t!("Templates")), false, Some(Message::Home(true)))
        ]
        .spacing(6)
        .boxed()
    });
    let groups = app.tree.iter().flat_map(|(_, tree)| tree.iter().flat_map(|p| p.groups.iter()));
    let bodies = groups.filter(|(s, _)| *s == "Bodies").flat_map(|(_, rows)| rows.iter());
    let bodies = bodies.map(|r| small(t, r.tip(), t.text));
    let l = app.session.display().length;
    let extent = m.bounds().map(|b| {
        let (lo, hi) = (b.min, b.max);
        let fmt = emcraft_ui_core::dialogs::short_number;
        kv(
            t,
            t!("extent"),
            format!("{} × {} × {} {}", fmt(l.from_si(hi.x - lo.x)), fmt(l.from_si(hi.y - lo.y)), fmt(l.from_si(hi.z - lo.z)), l.symbol()),
        )
    });
    let materials = m
        .materials
        .iter()
        .map(|mat| small(t, emcraft_render::math::to_unicode(&format!("{}:  {}", mat.name, emcraft_ui_core::math::material(mat))), t.text));
    let new = action(t, tr(t!("New material")), false, idle.then(|| send(Act::Fix("model.material.create".into()))));
    begin
        .into_iter()
        .chain(std::iter::once(section(t, t!("Bodies"))))
        .chain(bodies)
        .chain(extent)
        .chain(std::iter::once(section(t, t!("Materials"))))
        .chain(materials)
        .chain(std::iter::once(new))
        .collect()
}

/// The boundaries: the walls, their thickness, the open boundary, the symmetry planes set, the
/// group the model has and the share solved, a rotation, periodic boundaries.
fn boundaries(app: &App) -> Vec<Element<'_, Message>> {
    let t = app.look.tokens;
    let m = app.session.model();
    let l = app.session.display().length;
    let (fmt, lu) = (emcraft_ui_core::dialogs::short_number, l.symbol());
    let walls = kv(t, t!("walls"), m.domain.wall_conductivity.map_or_else(|| tr(t!("perfect conductor")).into(), |s| format!("σ = {s:.3e} S/m")));
    let thick = m.domain.wall_thickness.map(|w| kv(t, t!("wall thickness"), format!("{} {lu}", fmt(l.from_si(w.get())))));
    let open = m.domain.open.map_or_else(
        || tr(t!("closed (no PML)")).into(),
        |o| {
            let kind = tr(if o.shape == emcraft_model::OpenShape::Sphere { t!("spherical PML") } else { t!("PML") });
            trf(t!("{kind} {thickness} {unit} thick"), args!(kind = kind, thickness = fmt(l.from_si(o.thickness.get())), unit = lu))
        },
    );
    let planes: Vec<String> = m
        .domain
        .symmetry
        .iter()
        .map(|s| {
            let wall = tr(if s.kind == emcraft_model::SymmetryKind::Electric { t!("electric") } else { t!("magnetic") });
            format!("{} = {} {lu}, {wall}", s.axis.id(), fmt(l.from_si(s.at)))
        })
        .collect();
    let symmetry = kv(t, t!("symmetry"), if planes.is_empty() { tr(t!("none (the whole model)")).into() } else { planes.join("; ") });
    // The group the model has (its geometry and ports) and the share the planes set solve.
    let found = app.workflow.symmetry.as_ref().filter(|(g, _, _)| *g == app.generation);
    let group = found.map(|(_, found, _)| {
        use emcraft_model::symmetry::mirror_group;
        let has: Vec<&str> = found.iter().map(|f| f.axis.id()).collect();
        let axes = if has.is_empty() { tr(t!("no mirror plane")).to_owned() } else { trf(t!("mirrors across {axes}"), args!(axes = has.join(", "))) };
        let used: Vec<&str> = m.domain.symmetry.iter().map(|s| s.axis.id()).collect();
        let share = 1u32 << used.len().min(3);
        let solved = if used.is_empty() {
            trf(t!("the whole model ({group})"), args!(group = mirror_group(0)))
        } else {
            trf(t!("1/{share} of the model ({group})"), args!(share = share, group = mirror_group(used.len())))
        };
        let unused: Vec<&str> = has.iter().copied().filter(|a| !used.contains(a)).collect();
        let more = (!unused.is_empty() && used.len() < 2 && m.domain.rotation.is_none())
            .then(|| small(t, trf(t!("Find symmetry can cut it across {axes} too"), args!(axes = unused.join(", "))), t.text_dim));
        [kv(t, t!("model's symmetry"), format!("{} · {axes}", mirror_group(has.len()))), kv(t, t!("solved"), solved)]
            .into_iter()
            .chain(more)
            .collect::<Vec<_>>()
    });
    let rotation = match (&m.domain.rotation, found.and_then(|(_, _, r)| r.as_ref())) {
        (Some(r), _) => vec![kv(t, t!("rotation"), trf(t!("C{n} · 1/{n} of the model, harmonic {m}"), args!(n = r.n, m = r.m)))],
        (None, Some(r)) => vec![kv(t, t!("model's rotation"), r.group()), small(t, tr(t!("Find rotation can solve it on a sector")), t.text_dim)],
        (None, None) => Vec::new(),
    };
    let periodic = m.domain.periodic.as_ref().map(|p| kv(t, t!("periodic"), emcraft_ui_core::browser::periodic_text(p)));
    std::iter::once(section(t, t!("Boundaries")))
        .chain(std::iter::once(walls))
        .chain(thick)
        .chain(std::iter::once(kv(t, t!("open"), open)))
        .chain(std::iter::once(symmetry))
        .chain(group.into_iter().flatten())
        .chain(rotation)
        .chain(periodic)
        .collect()
}

/// The mesh's statistics (nodes, tetrahedra, boundary faces, the mean element and how many fit
/// in a wavelength at the band's top) and one solve's estimate.
fn mesh(app: &App) -> Vec<Element<'_, Message>> {
    let t = app.look.tokens;
    let l = app.session.display().length;
    let Some(m) = app.session.model().mesh.as_ref() else { return vec![section(t, t!("Mesh")), small(t, tr(t!("not meshed yet")), t.text_dim)] };
    let volume: f64 = (0..m.tets.len()).filter_map(|i| m.volume(i)).sum();
    let h = (6.0 * std::f64::consts::SQRT_2 * volume / m.tets.len().max(1) as f64).cbrt();
    let per = app.session.model().job.range().map(|b| {
        let index = app.session.model().materials.iter().map(|x| (x.eps_r.get() * x.mu_r.get()).sqrt()).fold(1.0, f64::max);
        let lambda = emcraft_model::units::C0 / (b.stop.get() * index);
        kv(t, t!("per wavelength"), trf(t!("{n} at the band's top"), args!(n = format!("{:.1}", lambda / h.max(1e-300)))))
    });
    let stats = [
        section(t, t!("Mesh")),
        kv(t, t!("nodes"), m.nodes.len().to_string()),
        kv(t, t!("tetrahedra"), m.tets.len().to_string()),
        kv(t, t!("boundary faces"), m.boundary.len().to_string()),
        kv(t, t!("mean element"), format!("{:.3} {}", l.from_si(h), l.symbol())),
    ];
    let estimate = match app.estimate() {
        Some(Ok(e)) => {
            let n = |k: &str| e.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
            let order = e.get("order").and_then(Value::as_u64).unwrap_or(2);
            let measured = e.get("from_this_session").and_then(Value::as_bool) == Some(true);
            let template = if measured { t!("{secs} s per frequency") } else { t!("{secs} s per frequency (reference machine)") };
            vec![
                kv(t, t!("unknowns"), trf(t!("{n} (order {order})"), args!(n = format!("{:.0}", n("unknowns")), order = order))),
                kv(t, t!("memory"), format!("{:.0} MB", n("memory_mb"))),
                kv(t, t!("time"), trf(template, args!(secs = format!("{:.2}", n("seconds_per_frequency"))))),
            ]
        }
        Some(Err(e)) => vec![small(t, e, t.text_dim)],
        None => Vec::new(),
    };
    stats.into_iter().chain(per).chain(std::iter::once(section(t, t!("Estimate")))).chain(estimate).collect()
}

/// The ports with their roles (an output may be made the input), the lumped ports and sources,
/// and the port-mode preview: a row a mode, which opens the Port modes page.
fn excitation(app: &App) -> Vec<Element<'_, Message>> {
    let t = app.look.tokens;
    let m = app.session.model();
    let l = app.session.display().length;
    let idle = app.job.is_none();
    let none = (m.ports.is_empty() && m.lumped.is_empty() && m.sources.is_empty()).then(|| small(t, tr(t!("none yet")), t.text_dim));
    let roles = m.ports.iter().zip(emcraft_ui_core::browser::port_roles(&app.session)).map(move |(p, role)| {
        let line = small(t, format!("{}  ·  {}", emcraft_ui_core::browser::port_text(p, l).shown, role.tip()), t.text);
        let drive = (!role.input).then(|| {
            let params = serde_json::json!({"port": role.name});
            action(
                t,
                tr(t!("Make the input")),
                false,
                idle.then(|| Message::Run(emcraft_ui_core::system::gesture::Command { id: "view.driven", params })),
            )
        });
        row(std::iter::once(line).chain(drive)).spacing(6).align_y(Alignment::Center).wrap().boxed()
    });
    let lumped =
        m.lumped.iter().map(move |p| small(t, trf(t!("Lumped port {name}: {impedance} Ω"), args!(name = p.name, impedance = p.impedance)), t.text));
    let sources =
        (!m.sources.is_empty()).then(|| small(t, plural(m.sources.len(), t!("{n} source"), t!("{n} sources"), args!(n = m.sources.len())), t.text));
    let head = std::iter::once(section(t, t!("Ports and sources"))).chain(none).chain(roles).chain(lumped).chain(sources);
    if m.ports.is_empty() {
        return head.collect();
    }
    let ready = m.mesh.is_some();
    let preview = action(t, tr(t!("Preview port modes")), true, (ready && idle).then(|| send(Act::PreviewModes)));
    let hint = (!ready).then(|| small(t, tr(t!("needs a mesh (step 6)")), t.text_dim));
    let rows = app.replies.port_modes.iter().flat_map(|reply| {
        reply.ports.iter().flat_map(move |p| {
            p.modes.iter().take(8).map(move |mode| {
                let numbers = if mode.propagating {
                    format!(
                        "β {}  εeff {}  Z {} Ω",
                        mode.beta.map_or_else(|| "—".into(), |v| format!("{v:.4}")),
                        mode.eps_eff.map_or_else(|| "—".into(), |v| format!("{v:.4}")),
                        mode.impedance.map_or_else(|| "—".into(), |v| format!("{v:.2}"))
                    )
                } else {
                    trf(t!("cut off, {a} dB/{unit}"), args!(a = format!("{:.1}", mode.attenuation_db.unwrap_or(f64::NAN)), unit = reply.length_unit))
                };
                button(text(format!("{} · {}   {numbers}", p.name, mode.name)).size(t.font_small))
                    .style(look::row(t, false))
                    .width(Fill)
                    .on_press(Message::Page(emcraft_ui_core::view::View::PortModes))
                    .boxed()
            })
        })
    });
    head.chain([section(t, t!("Port modes")), preview]).chain(hint).chain(rows).collect()
}

/// A Setup field: its name, its text (red with its error under it) and its unit.
fn field<'a>(app: &'a App, name: &'static str, label: Text, unit: &'a str) -> Element<'a, Message> {
    let t = app.look.tokens;
    let f = &app.workflow.fields;
    let error = f.shown_error(name);
    let input = text_input("", f.text(name)).size(t.font_body).padding([3, 6]).width(120).style(look::field(t));
    let input = input.on_input(move |s| send(Act::Typed(name, s))).on_submit(send(Act::Confirm));
    let line = row![text(tr(label)).size(t.font_small).color(t.text_dim).width(120), input, text(unit).size(t.font_small).color(t.text_dim)]
        .spacing(6)
        .align_y(Alignment::Center);
    column![line].push(error.map_or_else(|| Space::new().boxed(), |e| small(t, e, t.error))).spacing(2).boxed()
}

/// The run's settings for the kind chosen, over the band's diagram; the run's estimate; the
/// button that confirms them; the device it runs on.
fn setup(app: &App) -> Vec<Element<'_, Message>> {
    let t = app.look.tokens;
    let solver = app.solver();
    let (fu, lu) = (app.session.display().frequency.symbol(), app.session.display().length.symbol());
    let diagram = app.band_diagram().map(|d| canvas(crate::band::Band { diagram: d, tokens: t, unit: fu }).width(Fill).height(64).boxed());
    let fields: Vec<Element<Message>> = match solver {
        Solver::Frequency => {
            vec![field(app, "start", t!("start"), fu), field(app, "stop", t!("stop"), fu), field(app, "points", t!("Frequency points"), "")]
        }
        Solver::TimeDomain => {
            vec![
                field(app, "start", t!("start"), fu),
                field(app, "stop", t!("stop"), fu),
                field(app, "points", t!("Frequency points"), ""),
                field(app, "cell", t!("cell"), lu),
            ]
        }
        Solver::Eigenmodes => vec![field(app, "count", t!("modes"), ""), field(app, "frequency", t!("near"), fu)],
        Solver::Radiation => vec![field(app, "frequency", t!("frequency"), fu)],
        Solver::System => vec![
            field(app, "start", t!("start"), fu),
            field(app, "stop", t!("stop"), fu),
            field(app, "points", t!("Frequency points"), ""),
            field(app, "frequency", t!("budget at"), fu),
            small(t, tr(t!("Empty: the band's centre.")), t.text_dim),
        ],
        Solver::Pcb => Vec::new(),
    };
    let order = matches!(solver, Solver::Frequency | Solver::Eigenmodes | Solver::Radiation)
        .then(|| [field(app, "order", t!("Element order"), ""), small(t, tr(t!("Order 2: more accurate, more memory.")), t.text_dim)]);
    let estimate = (solver != Solver::System).then(|| match (app.session.model().mesh.is_some(), app.estimate()) {
        (false, _) => small(t, tr(t!("The run's time is estimated once the model is meshed (step 6).")), t.text_dim),
        (true, Some(e)) => estimate_line(t, e),
        (true, None) => Space::new().boxed(),
    });
    let confirm = if app.unconfirmed() {
        let ok = emcraft_ui_core::steps::form::checked(solver, app.session.limits().frequencies as u64, app.workflow.fields.clone()).0.is_ok();
        column![
            action(t, format!("✔ {}", tr(t!("Confirm settings"))), true, (ok && app.job.is_none()).then(|| send(Act::Confirm))),
            small(t, tr(t!("Not confirmed: the mesh and Run wait for these settings.")), t.warning)
        ]
        .spacing(4)
        .boxed()
    } else {
        small(t, format!("✔ {}", tr(t!("Settings confirmed"))), t.success)
    };
    let device = (solver != Solver::System).then(|| device(app));
    std::iter::once(section(t, t!("Settings")))
        .chain(diagram)
        .chain(fields)
        .chain(order.into_iter().flatten())
        .chain(estimate)
        .chain(std::iter::once(confirm))
        .chain(device.into_iter().flatten())
        .collect()
}

/// A run's estimate in a line: its time, `ref.` when it is the reference machine's.
fn estimate_line<'a>(t: Tokens, e: &Result<Value, String>) -> Element<'a, Message> {
    match e {
        Ok(e) => {
            let measured = e.get("basis").and_then(Value::as_str) == Some("measured");
            let time = emcraft_ui_core::job::duration(e.get("seconds").and_then(Value::as_f64).unwrap_or(f64::NAN));
            small(t, if measured { format!("⏱ ≈ {time}") } else { format!("⏱ ≈ {time} ({})", tr(t!("ref."))) }, t.text)
        }
        Err(e) => small(t, format!("⏱ {e}"), t.text_dim),
    }
}

/// Where the solve runs: the devices this machine lists (those it cannot run greyed) and the
/// precision, and what the next solve does.
fn device(app: &App) -> Vec<Element<'_, Message>> {
    let t = app.look.tokens;
    let none = serde_json::json!({});
    let rows = emcraft_ui_core::steps::devices::rows(app.workflow.devices.as_ref().unwrap_or(&none));
    let (current, precision) = (app.session.compute().device(), app.session.compute().precision);
    let idle = app.job.is_none();
    let devices = rows.iter().map(|d| {
        let p = if d.runs(precision) { precision } else { d.precisions.first().copied().unwrap_or(precision) };
        chip(t, d.device.id(), d.device == current, (d.available && idle).then(|| send(Act::Device(d.device, p))))
    });
    let on_cpu = current == Device::Cpu;
    let runs = |p: Precision| rows.iter().any(|d| d.device == current && d.runs(p));
    let precisions = Precision::ALL.into_iter().map(|p| {
        let on = if on_cpu { p == Precision::Double } else { precision == p };
        chip(t, lookup(p.id()), on, (!on_cpu && runs(p) && idle).then(|| send(Act::Device(current, p))))
    });
    let next = match app.solver() {
        Solver::TimeDomain if !on_cpu => t!("The next solve runs on {device} in {precision} precision."),
        Solver::TimeDomain => t!("The next solve runs on the CPU's threads (double precision)."),
        Solver::Frequency | Solver::Eigenmodes | Solver::Radiation if !on_cpu => {
            t!("The next solve runs on the CPU, its factorization's large fronts on {device} (complex double).")
        }
        _ => t!("The next solve runs on the CPU's threads."),
    };
    vec![
        section(t, t!("Compute device")),
        row(devices).spacing(6).boxed(),
        row(precisions).spacing(6).boxed(),
        small(t, trf(next, args!(device = current.id(), precision = lookup(precision.id()))), t.text),
    ]
}

/// Run: the preflight's findings each with the way to its step, the button, why it was refused
/// or failed, and its estimate.
fn run<'a>(app: &'a App, states: &[StepState]) -> Vec<Element<'a, Message>> {
    let t = app.look.tokens;
    if app.running() {
        return Vec::new();
    }
    let pre = emcraft_ui_core::steps::run::preflight(app.solver(), states);
    let ready = (pre.blocking.is_empty() && pre.warnings.is_empty()).then(|| small(t, tr(t!("Every step is ready.")), t.success));
    let go = |step: Step| action(t, trf(t!("Go to {step}"), args!(step = shown(step))), false, Some(Message::Step(Some(step))));
    let finding = |(step, said): &(Step, String), ink: Color| {
        row![text(format!("{} · {said}", shown(*step))).size(t.font_small).color(ink).width(Fill), go(*step)]
            .spacing(6)
            .align_y(Alignment::Center)
            .boxed()
    };
    let blocking = pre.blocking.iter().map(|f| finding(f, t.error));
    let warnings = pre.warnings.iter().map(|f| finding(f, t.warning));
    let solver = small(t, trf(t!("Solver: {solver}"), args!(solver = shown(app.solver()))), t.text);
    let button = crate::search::busy(app, action(t, format!("▶ {}", pre.button()), true, app.job.is_none().then(|| send(Act::Run))));
    let refused = app.workflow.refusal.as_ref().filter(|_| !pre.blocking.is_empty()).map(|_| {
        let n = pre.blocking.len();
        small(t, plural(n, t!("Run refused: {n} problem to fix first"), t!("Run refused: {n} problems to fix first"), args!(n = n)), t.error)
    });
    let failed = app.run_failure().and_then(|e| {
        let (command, _) = app.workflow.failure.as_ref()?;
        Some(small(t, format!("{}\n{e}", trf(t!("{command} failed"), args!(command = emcraft_ui_core::lang::command_label(command)))), t.error))
    });
    let estimate = app.estimate().map(|e| estimate_line(t, e));
    std::iter::once(section(t, t!("Preflight")))
        .chain(ready)
        .chain(blocking)
        .chain(warnings)
        .chain([solver, button])
        .chain(refused)
        .chain(failed)
        .chain(estimate)
        .collect()
}

/// The results: out of date, passivity and reciprocity, the mesh's convergence, the Touchstone
/// export.
fn results<'a>(app: &'a App, states: &[StepState]) -> Vec<Element<'a, Message>> {
    let t = app.look.tokens;
    let idle = app.job.is_none();
    let stale = states.get(Step::Results.index()).is_some_and(|s| s.status == Status::Stale);
    let stale_note = stale.then(|| {
        row![
            text(format!("↻ {}", tr(t!("Out of date")))).size(t.font_body).color(t.info),
            small(t, tr(t!("The model changed after this run.")), t.text_dim),
            action(t, tr(t!("Re-run")), true, idle.then(|| send(Act::Run)))
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .wrap()
        .boxed()
    });
    if !app.session.results().any() {
        return stale_note
            .into_iter()
            .chain(std::iter::once(small(t, tr(t!("No results yet: Run (step 6). The view tabs over the viewport show them.")), t.text_dim)))
            .collect();
    }
    if app.session.results().sweep.is_none() {
        return stale_note.into_iter().collect();
    }
    let check = app.workflow.check.as_ref().map(|c| {
        let n = |k: &str| c.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
        let passive = c.get("passive").and_then(Value::as_bool) == Some(true);
        column![
            kv(
                t,
                t!("largest σ(S)"),
                format!("{:.6} — {}", n("largest_singular_value"), tr(if passive { t!("passive") } else { t!("NOT passive") }))
            ),
            kv(t, t!("max |S − S^T|"), format!("{:.3e}", n("reciprocity_error")))
        ]
        .spacing(2)
        .boxed()
    });
    let f = &app.workflow.fields;
    let path =
        text_input("", f.text("touchstone")).size(t.font_body).padding([3, 6]).style(look::field(t)).on_input(|s| send(Act::Typed("touchstone", s)));
    let formats = [("ri", t!("real, imaginary")), ("ma", t!("magnitude, angle"))]
        .map(|(v, label)| chip(t, tr(label), f.text("format") == v, Some(send(Act::Format(v)))));
    let error = f.error("touchstone").map(|e| small(t, e, t.error));
    stale_note
        .into_iter()
        .chain([section(t, t!("Checks")), action(t, tr(t!("Passivity and reciprocity")), false, idle.then(|| send(Act::CheckResults)))])
        .chain(check)
        .chain([
            section(t, t!("Mesh accuracy")),
            small(t, tr(t!("Physics checks, not the mesh's error: refine and compare.")), t.text_dim),
            action(t, tr(t!("Set up mesh convergence")), false, (!stale && idle).then(|| send(Act::Fix("mesh.adapt".into())))),
            section(t, t!("Touchstone")),
            row![text(tr(t!("path"))).size(t.font_small).color(t.text_dim).width(Length::Shrink), path].spacing(6).align_y(Alignment::Center).boxed(),
            row(formats).spacing(6).boxed(),
        ])
        .chain(error)
        .chain(std::iter::once(action(t, tr(t!("Export")), true, idle.then(|| send(Act::Export)))))
        .collect()
}

/// The open panel's controls by their handles, for scenarios and agents: each check's Fix
/// (`step:fix:<i>`), Setup's Confirm, Run, the port-mode preview, the results' check and export.
pub fn handles(app: &App) -> Vec<(String, Message)> {
    let Some(step) = app.workflow.step.filter(|_| crate::steps::stepped(app)) else { return Vec::new() };
    let states = app.statuses();
    let fixes = states.get(step.index()).into_iter().flat_map(|s| {
        s.messages.iter().enumerate().filter_map(move |(i, m)| {
            let c = m.command.as_ref().filter(|c| fix_label(c, step).is_some())?;
            Some((format!("step:fix:{i}"), send(Act::Fix(c.clone()))))
        })
    });
    let own: Vec<(&str, Act)> = match step {
        Step::Setup => vec![("setup:confirm", Act::Confirm)],
        Step::Run => vec![("run:start", Act::Run)],
        Step::Excitation => vec![("excitation:preview", Act::PreviewModes)],
        Step::Results => vec![("results:check", Act::CheckResults), ("results:export", Act::Export), ("results:rerun", Act::Run)],
        _ => Vec::new(),
    };
    fixes.chain(own.into_iter().map(|(h, a)| (h.to_owned(), send(a)))).collect()
}
