//! The devices a solve may run on, as `solve.devices` lists them, and the one chosen for a
//! machine at first: an available CUDA accelerator in double precision (single for the time
//! domain when double is not offered), else the CPU's threads.

use emcraft_engine::{Device, Precision};
use serde_json::Value;

use super::Solver;

emcraft_macros::named_enum! {
    /// The device choice goes once from Pending to Automatic; any command of the person's
    /// makes it Explicit. Opening another project keeps the choice for this machine.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum DeviceSelection {
        #[default]
        Pending = "pending",
        Automatic = "automatic",
        Explicit = "explicit",
    }
}

/// A device as `solve.devices` lists it: whether this machine runs it, the precisions it runs
/// (in [`Precision::ALL`]'s order) and why it cannot run here.
#[derive(Clone, Debug, PartialEq)]
pub struct DeviceRow {
    pub device: Device,
    pub available: bool,
    pub precisions: Vec<Precision>,
    pub problem: Option<String>,
}

impl DeviceRow {
    pub fn runs(&self, p: Precision) -> bool {
        self.precisions.contains(&p)
    }
}

/// The devices of a `solve.devices` reply (names this build does not know are left out).
pub fn rows(list: &Value) -> Vec<DeviceRow> {
    let rows = list.get("devices").and_then(Value::as_array).map_or_default(Vec::as_slice);
    rows.iter()
        .filter_map(|d| {
            let device = d.get("name").and_then(Value::as_str).and_then(Device::parse)?;
            let listed = d.get("precisions").and_then(Value::as_array).map_or_default(Vec::as_slice);
            let precisions = Precision::ALL.into_iter().filter(|p| listed.iter().any(|v| v == p.id())).collect();
            let problem = d.get("problem").and_then(Value::as_str).map(str::to_owned);
            Some(DeviceRow { device, available: d.get("available").and_then(Value::as_bool) == Some(true), precisions, problem })
        })
        .collect()
}

/// The device and precision chosen for `solver` from a `solve.devices` reply.
///
/// ```
/// use emcraft_engine::{Device, Precision};
/// use emcraft_ui_core::steps::{Solver, devices::recommended};
/// let list = serde_json::json!({"devices":[{"name":"cpu","available":true,"precisions":["double"]},{"name":"cuda","available":true,"precisions":["single"]}]});
/// assert_eq!(recommended(&list, Solver::Frequency), (Device::Cpu, Precision::Double));
/// assert_eq!(recommended(&list, Solver::TimeDomain), (Device::Cuda, Precision::Single));
/// ```
pub fn recommended(list: &Value, solver: Solver) -> (Device, Precision) {
    use {Device::*, Precision::*};
    let candidates: &[(Device, Precision)] = match solver {
        Solver::TimeDomain => &[(Cuda, Double), (Cuda, Single), (Cpu, Double)],
        _ => &[(Cuda, Double), (Cpu, Double)],
    };
    let rows = rows(list);
    candidates.iter().copied().find(|&(device, p)| rows.iter().any(|r| r.device == device && r.available && r.runs(p))).unwrap_or((Cpu, Double))
}
