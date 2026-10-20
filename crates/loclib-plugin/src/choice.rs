//! A question a hook asks the user before it can answer (feature `choose`): a view command answers
//! [`crate::View::Choose`], an operation `{"choose": ...}` ([`crate::OpAnswer::Choose`]). The host
//! shows the items, the user picks one (or, `multi`, any of them), and the host runs the same hook
//! again with `params[param]` set to the item's value (a list of them).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
// Named apart from the sidecar's own `Choice`s in its contract.
#[schemars(rename = "PluginChoice")]
pub struct Choice {
    /// What's asked, in a line.
    pub title: String,
    /// The param the value chosen goes in: one of the hook's.
    pub param: String,
    /// Any of the items may be chosen, none too: the param is set to a list of their values.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub multi: bool,
    pub items: Vec<ChoiceItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ChoiceItem {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// What the param is set to when it's chosen (with `multi`, what its list has).
    pub value: Value,
}

impl Choice {
    /// Whether a host can ask it for a hook whose params `schema` describes: it says what it
    /// asks, and has items, each labelled and a value of a param the hook takes (with `multi`,
    /// an item of its list). Says what's wrong otherwise.
    pub fn check(&self, schema: Option<&Value>) -> Result<(), String> {
        if self.title.trim().is_empty() {
            return Err("its question has no title".into());
        }
        if self.items.is_empty() {
            return Err(format!("{:?} has nothing to choose from", self.title));
        }
        if let Some(i) = self.items.iter().position(|i| i.label.trim().is_empty()) {
            return Err(format!("{:?}'s item {i} has no label", self.title));
        }
        let Some(schema) = schema else { return Ok(()) };
        // The param alone, as the hook's schema takes it (by name or by a pattern), whatever else
        // it requires.
        let mut alone = schema.clone();
        if let Some(o) = alone.as_object_mut() {
            o.remove("required");
        }
        let fits = jsonschema::validator_for(&alone).map_err(|e| e.to_string())?;
        let given = |value: &Value| match self.multi {
            true => Value::Array(vec![value.clone()]),
            false => value.clone(),
        };
        let param = |i: &ChoiceItem| Value::Object(Map::from_iter([(self.param.clone(), given(&i.value))]));
        let unfit = self.items.iter().position(|i| !fits.is_valid(&param(i)));
        match unfit {
            Some(i) => {
                Err(format!("{:?}'s item {i} isn't a value of a param {:?} the hook takes", self.title, self.param))
            }
            None => Ok(()),
        }
    }

    /// `params` (an object, or null for none) with the param set to what was chosen: `chosen`,
    /// the items' values, the first of them unless `multi`.
    pub fn answered(&self, params: Value, chosen: Vec<Value>) -> Value {
        let mut params = match params {
            Value::Object(o) => o,
            _ => Map::new(),
        };
        let value = match self.multi {
            true => Value::Array(chosen),
            false => chosen.into_iter().next().unwrap_or_default(),
        };
        params.insert(self.param.clone(), value);
        Value::Object(params)
    }
}

#[cfg(test)]
mod tests;
