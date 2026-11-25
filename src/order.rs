//! The order (GLOSSARY.md "Order"): the engines, and of each the models, a
//! next session starts on, highest first; and the walk a pick takes through
//! it. No I/O: the caller hands in what `agy models` lists.
use crate::account::Engine;
use serde::{Deserialize, Serialize};

/// One engine of the order, and its models, highest first; none: the
/// engine's own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rank {
    pub engine: Engine,
    pub models: Vec<String>,
}

/// One (engine, model) a pick tries.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub engine: Engine,
    pub model: Option<String>,
}

/// Each `<engine>[@<model>[,<model>…]]`, highest first; each fault, with
/// its fix. agy's models are checked against `agy_models` (what `agy models`
/// lists, or why it failed), asked only when agy names any.
pub fn parse(
    specs: &[String],
    agy_models: impl FnOnce() -> Result<Vec<String>, String>,
) -> Result<Vec<Rank>, Vec<String>> {
    let mut order: Vec<Rank> = Vec::new();
    let mut faults = Vec::new();
    for spec in specs {
        let (name, models) = match spec.split_once('@') {
            Some((name, models)) => (name, Some(models)),
            None => (spec.as_str(), None),
        };
        let Ok(engine) = <Engine as clap::ValueEnum>::from_str(name, false) else {
            faults.push(format!(
                "no engine `{name}`: name one of {}",
                <Engine as clap::ValueEnum>::value_variants()
                    .iter()
                    .map(|e| e.name())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            continue;
        };
        if order.iter().any(|r| r.engine == engine) {
            faults.push(format!(
                "`{name}` is given twice: give an engine once, its models after its `@`, comma-separated"
            ));
            continue;
        }
        let models: Vec<String> = models
            .map(|m| m.split(',').map(|m| m.trim().to_string()).collect())
            .unwrap_or_default();
        if models.iter().any(String::is_empty) {
            faults.push(format!(
                "`{spec}` names no model: give `{name}@<model>`, or `{name}` alone for its own"
            ));
            continue;
        }
        if let Some(m) = models
            .iter()
            .enumerate()
            .find_map(|(i, m)| models[..i].contains(m).then_some(m))
        {
            faults.push(format!("`{spec}` gives `{m}` twice: give a model once"));
            continue;
        }
        order.push(Rank { engine, models });
    }
    if let Some(agy) = order
        .iter()
        .find(|r| r.engine == Engine::Agy && !r.models.is_empty())
    {
        match agy_models() {
            Err(e) => faults.push(e),
            Ok(listed) => {
                faults.extend(agy.models.iter().filter(|m| !listed.contains(m)).map(|m| {
                    format!(
                        "`agy models` lists no `{m}`: give one of {}",
                        listed.join(", ")
                    )
                }))
            }
        }
    }
    if faults.is_empty() {
        Ok(order)
    } else {
        Err(faults)
    }
}

/// The steps of a pick: of `order`, each engine's models in turn (the
/// engine's own where it lists none), narrowed to `engines` where any are
/// named and to `model` where it is; then each named engine no step of the
/// order tried, on `model`: an engine or model not in the order is tried
/// after it.
pub fn walk(order: &[Rank], engines: &[Engine], model: Option<&str>) -> Vec<Step> {
    let named = |e: Engine| engines.is_empty() || engines.contains(&e);
    let mut steps = Vec::new();
    for r in order.iter().filter(|r| named(r.engine)) {
        let models: Vec<Option<&str>> = match model {
            Some(m) if r.models.iter().any(|x| x == m) => vec![Some(m)],
            Some(_) => vec![],
            None if r.models.is_empty() => vec![None],
            None => r.models.iter().map(|m| Some(m.as_str())).collect(),
        };
        steps.extend(models.into_iter().map(|m| Step {
            engine: r.engine,
            model: m.map(str::to_string),
        }));
    }
    for &engine in engines {
        if !steps.iter().any(|s| s.engine == engine) {
            steps.push(Step {
                engine,
                model: model.map(str::to_string),
            });
        }
    }
    steps
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kit() -> Vec<Rank> {
        let specs = ["claude-code@opus", "codex", "agy@gemini,claude"];
        parse(&specs.map(String::from), || {
            Ok(vec!["claude".into(), "gemini".into()])
        })
        .unwrap()
    }

    fn shown(steps: &[Step]) -> Vec<String> {
        steps
            .iter()
            .map(|s| format!("{}@{}", s.engine.name(), s.model.as_deref().unwrap_or("-")))
            .collect()
    }

    #[test]
    fn a_spec_is_an_engine_and_its_models_highest_first() {
        assert_eq!(
            kit()[2],
            Rank {
                engine: Engine::Agy,
                models: vec!["gemini".into(), "claude".into()]
            }
        );
        let none = || -> Result<Vec<String>, String> { unreachable!("no agy model named") };
        let faults = parse(
            &["nope", "codex", "codex", "agy@a,", "kimi@a,a"].map(String::from),
            none,
        );
        let faults = faults.unwrap_err();
        assert_eq!(faults.len(), 4, "{faults:?}");
        let listed = || Ok(vec!["gemini".into()]);
        let faults = parse(&["agy@gemini,x,y"].map(String::from), listed).unwrap_err();
        assert_eq!(faults.len(), 2, "{faults:?}");
    }

    #[test]
    fn the_walk_takes_each_engine_s_models_in_turn_narrowed_to_what_is_named() {
        let all = walk(&kit(), &[], None);
        assert_eq!(
            shown(&all),
            ["claude-code@opus", "codex@-", "agy@gemini", "agy@claude"]
        );
        let named = walk(
            &kit(),
            &[Engine::Kimi, Engine::Agy, Engine::ClaudeCode],
            None,
        );
        assert_eq!(
            shown(&named),
            ["claude-code@opus", "agy@gemini", "agy@claude", "kimi@-"]
        );
        assert_eq!(shown(&walk(&kit(), &[], Some("claude"))), ["agy@claude"]);
        let other = walk(&kit(), &[Engine::Codex, Engine::Agy], Some("claude"));
        assert_eq!(shown(&other), ["agy@claude", "codex@claude"]);
        assert!(walk(&kit(), &[], Some("gpt")).is_empty());
        assert!(walk(&[], &[], None).is_empty());
    }
}
