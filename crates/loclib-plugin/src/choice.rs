//! A question a hook asks the user before it can answer (feature `choose`): a view command answers
//! [`crate::View::Choose`], an operation `{"choose": ...}` ([`crate::OpAnswer::Choose`]). The host
//! shows the items, the user picks one (or, `multi`, any of them), and the host runs the same hook
//! again with `params[param]` set to the item's value (a list of them). Further questions in
//! `more` are asked in the same round, and the hook run again once with every param set.

use rustc_hash::FxHashSet;
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
    /// Questions asked after this one before the hook runs again, each about a param of its own
    /// (and asking none of its own). A host that doesn't know them asks this one alone, so a hook
    /// still asks what's left when it's run again.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub more: Vec<Choice>,
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
    /// How many questions one round may ask, this one and `more`: enough for every entry of a
    /// long set's tracklist (a three-hour set runs to about 60 tracks), while the user doesn't
    /// sit through more than that before the hook hears their answers.
    pub const MAX_QUESTIONS: usize = 64;

    /// `questions` asked in one round, the first with the rest as its `more`: at most
    /// [`Choice::MAX_QUESTIONS`] of them, those past it left for the round after. `None` for
    /// none.
    pub fn together(questions: impl IntoIterator<Item = Choice>) -> Option<Choice> {
        let mut questions = questions.into_iter().take(Self::MAX_QUESTIONS);
        let first = questions.next()?;
        Some(Choice { more: questions.collect(), ..first })
    }

    /// This question, then those in `more`, in the order they're asked.
    pub fn questions(&self) -> impl Iterator<Item = &Choice> {
        std::iter::once(self).chain(&self.more)
    }

    /// Whether a host can ask it for a hook whose params `schema` describes: at most
    /// [`Choice::MAX_QUESTIONS`] questions, each about a param of its own, saying what it asks,
    /// and with items, each labelled and a value of a param the hook takes (with `multi`, an item
    /// of its list). Says what's wrong otherwise.
    pub fn check(&self, schema: Option<&Value>) -> Result<(), String> {
        let asked = self.more.len() + 1;
        if asked > Self::MAX_QUESTIONS {
            return Err(format!("it asks {asked} questions at once, more than {}", Self::MAX_QUESTIONS));
        }
        if let Some(q) = self.more.iter().find(|q| !q.more.is_empty()) {
            return Err(format!("{:?} asks more questions of its own", q.title));
        }
        let mut params = FxHashSet::default();
        if let Some(q) = self.questions().find(|q| !params.insert(&q.param)) {
            return Err(format!("{:?} asks for {:?} again", q.title, q.param));
        }
        // The param alone, as the hook's schema takes it (by name or by a pattern), whatever else
        // it requires.
        let alone = |schema: &Value| {
            let mut alone = schema.clone();
            if let Some(o) = alone.as_object_mut() {
                o.remove("required");
            }
            jsonschema::validator_for(&alone).map_err(|e| e.to_string())
        };
        let fits = schema.map(alone).transpose()?;
        self.questions().try_for_each(|q| q.check_one(fits.as_ref()))
    }

    /// Whether this question alone can be asked, its param's values `fits` (see [`Choice::check`]).
    fn check_one(&self, fits: Option<&jsonschema::Validator>) -> Result<(), String> {
        if self.title.trim().is_empty() {
            return Err("its question has no title".into());
        }
        if self.items.is_empty() {
            return Err(format!("{:?} has nothing to choose from", self.title));
        }
        if let Some(i) = self.items.iter().position(|i| i.label.trim().is_empty()) {
            return Err(format!("{:?}'s item {i} has no label", self.title));
        }
        let Some(fits) = fits else { return Ok(()) };
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

    /// `params` (an object, or null for none) with each question's param set to what was chosen
    /// for it: `chosen` has an answer for each of [`Choice::questions`] in turn, the items'
    /// values, the first of them unless `multi`. A question without an answer there leaves its
    /// param as it was.
    pub fn answered(&self, params: Value, chosen: Vec<Vec<Value>>) -> Value {
        let mut params = match params {
            Value::Object(o) => o,
            _ => Map::new(),
        };
        for (q, chosen) in self.questions().zip(chosen) {
            let value = match q.multi {
                true => Value::Array(chosen),
                false => chosen.into_iter().next().unwrap_or_default(),
            };
            params.insert(q.param.clone(), value);
        }
        Value::Object(params)
    }
}

#[cfg(test)]
mod tests;
