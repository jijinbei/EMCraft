//! `emcraft-cli`: EMCraft without a window.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::error::Error;
use std::io::Write;
use std::process::ExitCode;

use emcraft_engine::{COMMANDS, Session, features_markdown};
use serde_json::{Value, json};

/// Whatever stopped a command, said once at exit.
type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Writes a line to stdout: a stdout that cannot take it (a pipe closed early, `| head`) ends
/// the command with an error instead of the panic `println!` gives.
macro_rules! out {
    ($($t:tt)*) => {
        writeln!(std::io::stdout().lock(), $($t)*)?
    };
}

/// [`out!`] without the line's end.
macro_rules! put {
    ($($t:tt)*) => {
        write!(std::io::stdout().lock(), $($t)*)?
    };
}

const USAGE: &str = "\
usage: emcraft-cli <command>

  commands                 list every command with its parameters
  features                 the command reference as Markdown (docs/features.md)
  run SCRIPT.json          run a command script (a JSON array of {\"command\", \"params\"})
                           and print each reply and the model summary
  exec COMMAND [JSON]      run one command on an empty session and print its reply
  sample NAME [--out DIR] [--frequency GHZ]...
                           open and solve a sample, print its S-parameters against the
                           exact solution, and write its pictures to DIR (default .):
                           s-parameters.png and the H-plane, E-plane and cross-section
                           fields at each frequency (default 6 and 8 GHz); an eigenmode
                           sample (cavity) prints its modes against the closed form and
                           writes each mode's pictures
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = (|| -> Result<()> {
        match args.first().map(String::as_str) {
            Some("commands") => {
                for c in COMMANDS {
                    out!("{:<24} {}", c.id, c.label);
                    for p in c.params {
                        out!("    {:<14} {:<8} {}{}", p.name, p.kind, if p.required { "(required) " } else { "" }, p.doc);
                    }
                }
                Ok(())
            }
            Some("features") => {
                put!("{}", features_markdown());
                Ok(())
            }
            Some("run") => run(args.get(1)),
            Some("exec") => exec(args.get(1), args.get(2)),
            Some("sample") => sample(&args[1..]),
            Some("-h" | "--help" | "help") | None => {
                put!("{USAGE}");
                Ok(())
            }
            Some(other) => Err(format!("unknown command `{other}`\n\n{USAGE}").into()),
        }
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        // A reader that stopped reading (`| head`) has what it wanted.
        Err(e) if e.downcast_ref::<std::io::Error>().is_some_and(|io| io.kind() == std::io::ErrorKind::BrokenPipe) => ExitCode::SUCCESS,
        Err(e) => {
            // Nothing more to do if stderr cannot take it either.
            let _ = writeln!(std::io::stderr().lock(), "error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
}

fn run(path: Option<&String>) -> Result<()> {
    let path = path.ok_or("run: give a script path")?;
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let script: Value = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
    let mut s = Session::new();
    let replies = s.run_script(&script)?;
    for r in &replies {
        out!("{}", pretty(r));
    }
    out!("{}", pretty(&s.summary()));
    Ok(())
}

fn exec(id: Option<&String>, params: Option<&String>) -> Result<()> {
    let id = id.ok_or("exec: give a command id")?;
    let params: Value = match params {
        Some(p) => serde_json::from_str(p).map_err(|e| format!("parameters: {e}"))?,
        None => json!({}),
    };
    let mut s = Session::new();
    let r = s.execute(id, &params)?;
    out!("{}", pretty(&r));
    Ok(())
}

/// An eigenmode sample: its modes against the closed form, and each mode's pictures.
fn eigen_sample(name: &str, out: &str) -> Result<()> {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": name, "solve": true}))?;
    let t = s.execute("results.eigenmodes", &json!({}))?;
    out!("{:>5}  {:>12} {:>12} {:>8}  {:>9}", "mode", "f (GHz)", "exact", "exact", "Q");
    let exact = t["exact"].as_array().cloned().unwrap_or_default();
    for (i, m) in t["modes"].as_array().into_iter().flatten().enumerate() {
        let f = m["frequency"].as_f64().unwrap_or(f64::NAN);
        let x = exact.get(i);
        out!(
            "{:>5}  {:>12.6} {:>12.6} {:>8}  {:>9.1}",
            i + 1,
            f,
            x.and_then(|x| x["frequency"].as_f64()).unwrap_or(f64::NAN),
            x.and_then(|x| x["mode"].as_str()).unwrap_or(""),
            m["q"].as_f64().unwrap_or(f64::NAN)
        );
    }
    for i in 1..=t["modes"].as_array().map_or(0, Vec::len) {
        for plane in ["h", "e", "cross"] {
            let path = format!("{out}/mode{i}-{plane}.png");
            s.execute("view.field", &json!({"path": path, "mode": i, "plane": plane, "component": "|e|"}))?;
            out!("wrote {path}");
        }
        // A standing wave: H peaks a quarter period after E.
        for plane in ["h", "cross"] {
            let path = format!("{out}/mode{i}-{plane}-90deg.png");
            s.execute("view.field", &json!({"path": path, "mode": i, "plane": plane, "phase": 90, "component": "|h|"}))?;
            out!("wrote {path}");
        }
    }
    Ok(())
}

fn sample(args: &[String]) -> Result<()> {
    let name = args.first().ok_or("sample: give a sample name (highpass)")?;
    let mut out = ".".to_owned();
    let mut freqs: Vec<f64> = Vec::new();
    let mut it = args[1..].iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--out" => out = it.next().ok_or("--out needs a directory")?.clone(),
            "--frequency" => freqs.push(it.next().and_then(|f| f.parse().ok()).ok_or("--frequency needs a number (GHz)")?),
            other => return Err(format!("sample: unknown option `{other}`").into()),
        }
    }
    if freqs.is_empty() {
        freqs = vec![6.0, 8.0];
    }
    std::fs::create_dir_all(&out).map_err(|e| format!("{out}: {e}"))?;
    let mut s = Session::new();
    let script: Value = emcraft_engine::sample::find(name).and_then(|x| serde_json::from_str(x.script).ok()).unwrap_or(Value::Null);
    if script.as_array().is_some_and(|steps| steps.iter().any(|st| st["command"] == "solve.eigenmodes")) {
        return eigen_sample(name, &out);
    }
    s.execute("file.sample", &json!({"name": name}))?;
    // The sample's own sweep, with the pictures' frequencies added.
    let mut solve = script
        .as_array()
        .and_then(|steps| steps.iter().find(|st| st["command"] == "solve.frequency"))
        .map(|st| st["params"].clone())
        .unwrap_or_else(|| json!({"order": 2}));
    let mut all = freqs.clone();
    if let (Some(a), Some(b)) = (solve["start"].as_f64(), solve["stop"].as_f64()) {
        let n = solve["points"].as_u64().unwrap_or(11).max(2);
        all.extend((0..n).map(|k| a + (b - a) * k as f64 / (n - 1) as f64));
    }
    if let Some(o) = solve.as_object_mut() {
        o.retain(|k, _| k == "order");
        o.insert("frequencies".into(), json!(all));
    }
    s.execute("solve.frequency", &solve)?;
    let cmp = s.execute("results.compare", &json!({}))?;
    out!("{:>9}  {:>10} {:>10}  {:>10} {:>10}  {:>9} {:>9}", "f (GHz)", "|S21| dB", "exact", "|S11| dB", "exact", "err S21", "err S11");
    for p in cmp["points"].as_array().into_iter().flatten() {
        out!(
            "{:>9.3}  {:>10.4} {:>10.4}  {:>10.4} {:>10.4}  {:>9.2e} {:>9.2e}",
            p["frequency"].as_f64().unwrap_or(f64::NAN),
            p["s21"]["db"].as_f64().unwrap_or(f64::NAN),
            p["exact_s21"]["db"].as_f64().unwrap_or(f64::NAN),
            p["s11"]["db"].as_f64().unwrap_or(f64::NAN),
            p["exact_s11"]["db"].as_f64().unwrap_or(f64::NAN),
            p["error_s21"].as_f64().unwrap_or(f64::NAN),
            p["error_s11"].as_f64().unwrap_or(f64::NAN),
        );
    }
    out!("worst |S - S_exact| = {:.3e}", cmp["worst"].as_f64().unwrap_or(f64::NAN));
    let path = format!("{out}/s-parameters.png");
    s.execute("view.sparameters", &json!({"path": path}))?;
    out!("wrote {path}");
    for f in &freqs {
        for plane in ["h", "e", "cross"] {
            let path = format!("{out}/{plane}-{f}GHz.png");
            s.execute("view.field", &json!({"path": path, "frequency": f, "plane": plane}))?;
            out!("wrote {path}");
        }
    }
    Ok(())
}
