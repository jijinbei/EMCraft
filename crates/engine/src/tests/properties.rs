//! Property tests: every command, given random values for its own parameters, answers with a
//! reply or an error, never a panic. A fixed seed and no saved failures keep the gate
//! deterministic.

use proptest::prelude::*;
use proptest::test_runner::{Config, RngSeed};
use serde_json::{Map, Value, json};

use crate::*;

/// A hostile value: extreme or odd numbers, strings (some a parameter's own choices), arrays
/// and objects of those, or the wrong kind altogether.
fn value() -> impl Strategy<Value = Value> {
    let number =
        prop_oneof![Just(0.0), Just(-1.0), Just(1e308), Just(-1e308), Just(f64::MIN_POSITIVE), Just(1e12), -1e3..1e3f64,].prop_map(|x| json!(x));
    let leaf = prop_oneof![
        number,
        Just(json!(null)),
        Just(json!(true)),
        Just(json!("")),
        Just(json!("x")),
        Just(json!("cpu")),
        Just(json!("1")),
        (0u64..20).prop_map(|n| json!(n)),
    ];
    leaf.prop_recursive(2, 8, 4, |inner| {
        prop_oneof![prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Array), inner.prop_map(|v| json!({"name": v, "value": v}))]
    })
}

/// A command (not one that reads or writes files) and random values for some of its parameters.
fn call() -> impl Strategy<Value = (&'static CommandSpec, Value)> {
    let commands: Vec<&'static CommandSpec> = COMMANDS.iter().filter(|c| !c.params.iter().any(|p| p.name == "path")).collect();
    prop::sample::select(commands).prop_flat_map(|c| {
        // A command without parameters gets made-up names, which it must refuse.
        let names: Vec<&'static str> = match c.params {
            [] => vec!["bogus"],
            ps => ps.iter().map(|p| p.name).collect(),
        };
        let params = prop::collection::vec((prop::sample::select(names), value()), 0..5);
        params.prop_map(move |kv| (c, Value::Object(kv.into_iter().map(|(k, v)| (k.to_owned(), v)).collect::<Map<_, _>>())))
    })
}

proptest! {
    #![proptest_config(Config { cases: 512, rng_seed: RngSeed::Fixed(0x454d_4372), failure_persistence: None, ..Config::default() })]

    /// On an empty session and on a sample's model (not meshed, so nothing solves).
    #[test]
    fn random_parameters_never_panic((c, params) in call(), sample in prop::sample::select(vec!["", "highpass", "pcb-parallel"])) {
        let mut s = Session::new();
        if !sample.is_empty() {
            s.execute("file.sample", &json!({"name": sample})).map_err(|e| TestCaseError::fail(e.to_string()))?;
        }
        let r = s.execute(c.id, &params);
        prop_assert!(!matches!(r, Err(EngineError::Panicked(_))), "{} panicked on {params} ({sample})", c.id);
    }
}
