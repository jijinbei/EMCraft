//! The project file, `.emcraft` version 0.1 (docs/project-file.md): a package
//! ([`emcraft_io::package`]) of `project.json` — the history that builds the model, the runs,
//! the views, the analysis history and the model for other tools — and the arrays of numbers
//! they name, little-endian.
//!
//! A file is hostile: everything read is checked (sizes capped and agreeing, numbers finite,
//! commands known), and a file of another format or version is refused whole.

use std::path::Path;

use emcraft_io::package::{Reader, Writer};
use emcraft_model::Model;
use emcraft_solver::Complex64 as C;
use serde::{Deserialize, Serialize};

pub mod mesh;
pub mod results;

use crate::archive::file::StoredHistory;
use crate::runs::Run;
use crate::{EngineError, Result, SData, Session, Step};

/// What is wrong with a part of a project file: the part (a result, the mesh, a kept solve) and
/// why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileError {
    pub part: String,
    pub why: String,
}

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.part, self.why)
    }
}

/// What a project file's `format` says.
pub const FORMAT: &str = "emcraft-project";

/// The version of project files written and read.
pub const VERSION: &str = "0.1";

/// The part holding the project's JSON.
const PROJECT: &str = "project.json";

/// Most bytes of `project.json`.
const MAX_PROJECT_JSON: u64 = 64 << 20;

/// `project.json`.
#[derive(Serialize, Deserialize)]
struct ProjectFile<'m> {
    format: String,
    version: String,
    /// The EMCraft that wrote it.
    emcraft: String,
    history: Vec<Step>,
    runs: Vec<Run>,
    views: crate::views::Views,
    analysis_history: StoredHistory,
    /// The model's mesh, its numbers array parts.
    mesh: Option<mesh::StoredMesh>,
    /// The session's results, their numbers array parts.
    results: results::StoredSession,
    /// For other tools to read; opening replays the history and does not read it.
    #[serde(skip_deserializing)]
    model: Option<&'m Model>,
}

/// What a project file holds besides its model: the history, the runs, the views and the
/// analysis history (its solves and the last id given).
pub(crate) struct Stored {
    pub history: Vec<Step>,
    pub runs: Vec<Run>,
    pub views: crate::views::Views,
    pub archive: Vec<crate::archive::Archived>,
    pub archive_next: u32,
    /// The model's mesh, read once the model is built ([`Stored::mesh`]).
    mesh: Option<mesh::StoredMesh>,
    /// The results, read once the model and its mesh are ([`Stored::results`]).
    results: Option<results::StoredSession>,
    /// The package, for the parts read once the model is built.
    package: Option<Reader>,
}

impl Stored {
    /// What a project that cannot be read is saved as: unlike anything (no command has this
    /// history's id, so no history equals it).
    pub(crate) fn unreadable() -> Stored {
        Stored {
            history: vec![Step::new("(the original could not be read)", serde_json::json!({}))],
            runs: Vec::new(),
            views: crate::views::Views::default(),
            archive: Vec::new(),
            archive_next: 0,
            mesh: None,
            results: None,
            package: None,
        }
    }

    /// The file's results for `model` (its mesh on): those that fit, the stale ones marked,
    /// the kept solve they are, and why any was left out.
    pub(crate) fn results(&mut self, model: &Model) -> (crate::Results, Option<u32>, Vec<FileError>) {
        match (self.results.take(), self.package.as_mut()) {
            (Some(st), Some(r)) => {
                let (stale, archived) = (st.stale.clone(), st.archive_current);
                let (mut results, dropped) = st.read(r, model);
                results.mark_stale(stale);
                (results, archived, dropped)
            }
            _ => (crate::Results::default(), None, Vec::new()),
        }
    }

    /// The file's mesh for a model of `materials` materials (none when the model had none),
    /// checked.
    pub(crate) fn mesh(&mut self, materials: usize) -> std::result::Result<Option<emcraft_model::Mesh>, FileError> {
        match (self.mesh.take(), self.package.as_mut()) {
            (Some(m), Some(r)) => m.read(r, materials).map(Some).map_err(|why| FileError { part: "its mesh".into(), why }),
            _ => Ok(None),
        }
    }
}

fn failed(path: &Path, e: impl std::fmt::Display) -> EngineError {
    EngineError::failed(format!("`{}`: {e}", path.display()))
}

/// Writes `s`'s project to `path`: to a temporary file beside it, renamed over it, so a crash
/// or a full disk leaves the old file whole.
pub(crate) fn write(s: &Session, path: &Path, fields: bool) -> Result<bool> {
    let temp = path.with_extension(format!("{}.tmp", path.extension().and_then(|e| e.to_str()).unwrap_or(crate::project::EXTENSION)));
    let written = write_to(s, &temp, fields);
    let renamed = written.and_then(|fields| {
        std::fs::rename(&temp, path).map_err(|e| EngineError::cannot_write(&path.display().to_string(), e))?;
        Ok(fields)
    });
    if renamed.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    renamed
}

/// Writes the package; whether the fields are in it.
fn write_to(s: &Session, path: &Path, fields: bool) -> Result<bool> {
    let mut w = Writer::create(path).map_err(|e| failed(path, e))?;
    let analysis_history = crate::archive::file::stored(&s.archive, s.archive_next, &mut w).map_err(|e| failed(path, e))?;
    let mesh = s.model.mesh.as_deref().map(|m| mesh::StoredMesh::write(m, &mut w)).transpose().map_err(|e| failed(path, e))?;
    let results = results::StoredSession::write(s, fields, &mut w).map_err(|e| failed(path, e))?;
    let fields = results.fields;
    let file = ProjectFile {
        format: FORMAT.to_owned(),
        version: VERSION.to_owned(),
        emcraft: env!("CARGO_PKG_VERSION").to_owned(),
        history: s.history.clone(),
        runs: s.runs.clone(),
        views: s.views.clone(),
        analysis_history,
        mesh,
        results,
        model: Some(&s.model),
    };
    // Through a JSON value, whose objects keep their keys sorted: the text does not depend on
    // the order the fields are declared in.
    let doc = serde_json::to_value(&file).map_err(EngineError::failed)?;
    w.text(PROJECT, &crate::project::pretty(&doc)?).map_err(|e| failed(path, e))?;
    w.finish().map_err(|e| failed(path, e))?;
    Ok(fields)
}

/// The project file at `path`, read and checked.
pub(crate) fn read(path: &Path) -> Result<Stored> {
    let mut r = Reader::open(path).map_err(|e| failed(path, format!("not an EMCraft project ({e})")))?;
    let text = r.text(PROJECT, MAX_PROJECT_JSON).map_err(|e| failed(path, format!("not an EMCraft project ({e})")))?;
    let doc: serde_json::Value = serde_json::from_str(&text).map_err(|e| failed(path, format!("not a project file ({e})")))?;
    if doc.get("format").and_then(serde_json::Value::as_str) != Some(FORMAT) {
        return Err(failed(path, "not an EMCraft project"));
    }
    match doc.get("version").and_then(serde_json::Value::as_str) {
        Some(VERSION) => {}
        v => {
            let v = v.map_or_else(|| "none".to_owned(), |v| crate::text::clip(v, 16));
            return Err(failed(path, format!("a project of version {v}; this EMCraft reads version {VERSION}")));
        }
    }
    let file: ProjectFile<'_> = serde_json::from_value(doc).map_err(|e| failed(path, format!("not a project file ({e})")))?;
    let runs = crate::runs::checked(file.runs).map_err(|e| failed(path, e))?;
    let views = file.views.checked().map_err(|e| failed(path, e))?;
    let (archive, archive_next) = crate::archive::file::read(file.analysis_history, &mut r).map_err(|e| failed(path, e))?;
    Ok(Stored { history: file.history, runs, views, archive, archive_next, mesh: file.mesh, results: Some(file.results), package: Some(r) })
}

/// Complex values as an array part: real then imaginary, one after the other.
pub(crate) fn put_complex(w: &mut Writer, name: &str, values: &[C]) -> std::result::Result<(), String> {
    w.values(name, values.iter().flat_map(|c| [c.re, c.im])).map_err(|e| e.to_string())
}

/// `count` complex values of array part `name`: exactly that many, every one finite.
pub(crate) fn get_complex(r: &mut Reader, name: &str, count: usize) -> std::result::Result<Vec<C>, String> {
    let numbers: Vec<f64> = r.values(name, count.saturating_mul(2)).map_err(|e| e.to_string())?;
    if numbers.len() != 2 * count {
        return Err(format!("{name}: {} values, not {count}", numbers.len() / 2));
    }
    if !numbers.iter().all(|x| x.is_finite()) {
        return Err(format!("{name}: values must be finite numbers"));
    }
    let (pairs, _) = numbers.as_chunks::<2>();
    Ok(pairs.iter().map(|&[re, im]| C::new(re, im)).collect())
}

/// S-parameters in a file: the frequencies and S's shape in `project.json`, the values an array
/// part.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredS {
    /// Ascending (Hz).
    pub frequencies: Vec<f64>,
    /// S's rows.
    pub outputs: usize,
    /// The excited entries (S's columns).
    pub inputs: Vec<usize>,
    /// The array part of the values (frequency by frequency, row by row).
    pub values: String,
}

impl StoredS {
    /// `s` written as part `name`.
    pub(crate) fn write(s: &SData, name: &str, w: &mut Writer) -> std::result::Result<StoredS, String> {
        put_complex(w, name, s.values())?;
        Ok(StoredS { frequencies: s.frequencies().to_vec(), outputs: s.outputs(), inputs: s.inputs().to_vec(), values: name.to_owned() })
    }

    /// The S-parameters, checked.
    pub(crate) fn read(self, r: &mut Reader) -> std::result::Result<SData, String> {
        let per = self.outputs.saturating_mul(self.inputs.len());
        if self.frequencies.len().saturating_mul(per) > crate::sdata::MAX_VALUES {
            return Err(format!("S-parameters of more than {} values", crate::sdata::MAX_VALUES));
        }
        let values = get_complex(r, &self.values, self.frequencies.len() * per)?;
        SData::new(self.frequencies, self.outputs, self.inputs, values)
    }
}
