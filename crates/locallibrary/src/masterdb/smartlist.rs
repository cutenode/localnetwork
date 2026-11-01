//! rekordbox's intelligent playlists: the rules `djmdPlaylist.SmartList` holds, as pyrekordbox
//! reads them (`pyrekordbox/masterdb/smartlist.py`). The column is XML: a `NODE` whose
//! `LogicalOperator` is 1 (match all) or 2 (match any), with a `CONDITION` per rule naming a
//! `PropertyName` (`artist`, `bpm`, `myTag`...), an `Operator` (1 is, 2 isn't, 3 greater than,
//! 4 less than, 5 in range, 6 in the last, 7 not in the last, 8 contains, 9 doesn't contain,
//! 10 starts with, 11 ends with), `ValueLeft`, `ValueRight` (a range's end) and `ValueUnit`
//! (`day`, `week`, `month` or `year`, for 6 and 7).
//!
//! The values are the `djmdContent` columns' as [`crate::masterdb`] reads them: text for names,
//! stars for `rating`, seconds for `duration`, `YYYY-MM-DD` for dates, a colour row (1 pink ... 8
//! purple, 0 none) for `grouping`, and a `djmdMyTag` id for `myTag` (written as a signed 32-bit
//! number when it's above 2^31). A `bpm` value of 1000 or more is taken to be in hundredths, as
//! `djmdContent.BPM` is (inferred: pyrekordbox compares it with that column as it is).
//!
//! [`SmartList::matches`] says which tracks a list holds, text compared as [`crate::query`]'s
//! filters compare it (ignoring case and runs of spaces). [`SmartList::filter`] writes the lists
//! a filter can say as one, matching the same tracks: text that contains (or doesn't) a value for
//! the fields a filter has on their own, numbers, the date added, colours and My Tags, and match
//! any of several values of one field. An artist (a filter's also matches the album artist), a
//! whole value, a start or an end, a date relative to today, and fields filters don't have can't
//! be said.

use chrono::{Months, NaiveDate};

use crate::model::{Track, TrackColor, normalize};
use crate::query::date_value;

/// An intelligent playlist's rules.
#[derive(Debug, Clone, PartialEq)]
pub struct SmartList {
    /// A track matches any one condition, rather than all of them.
    pub any: bool,
    pub conditions: Vec<Condition>,
}

/// One rule of an intelligent playlist.
#[derive(Debug, Clone, PartialEq)]
pub struct Condition {
    /// The rule as rekordbox stores it, for messages: `mixName contains "dub"`.
    pub text: String,
    /// `None` when it can't be evaluated: a property or operator this doesn't know, a value that
    /// can't be read, or a column [`crate::masterdb`] doesn't read (`dateCreated`).
    pub rule: Option<Rule>,
}

/// A condition as it's evaluated.
#[derive(Debug, Clone, PartialEq)]
pub enum Rule {
    Text {
        field: TextField,
        op: TextOp,
        value: String,
    },
    Number {
        field: NumberField,
        op: Compare<f64>,
    },
    Date {
        field: DateField,
        op: DateOp,
    },
    Color {
        equal: bool,
        color: Option<TrackColor>,
    },
    /// Has (or hasn't) the My Tag at this `Category/Tag` path; empty for a tag that's gone.
    Tag {
        has: bool,
        path: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextField {
    Artist,
    Album,
    AlbumArtist,
    OriginalArtist,
    Producer,
    Genre,
    Key,
    Label,
    MixName,
    Remixer,
    Title,
    Comment,
    FileName,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextOp {
    Is,
    IsNot,
    Contains,
    NotContains,
    StartsWith,
    EndsWith,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberField {
    Bpm,
    Rating,
    Plays,
    Year,
    Duration,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Compare<T> {
    Is(T),
    IsNot(T),
    Greater(T),
    Less(T),
    /// Both ends included, in either order.
    Range(T, T),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateField {
    /// `stockDate`: when it was added to the collection.
    Added,
    Released,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DateOp {
    /// Dates as `YYYYMMDD` numbers, as `query::date_value` makes them.
    Compare(Compare<f64>),
    /// Within the last `n` days, weeks, months or years (or not).
    InLast { n: u32, unit: Unit, within: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Day,
    Week,
    Month,
    Year,
}

/// Colour names as filters take them, in rekordbox's order (1 pink ... 8 purple).
const COLORS: [&str; 8] = ["pink", "red", "orange", "yellow", "green", "aqua", "blue", "purple"];

impl SmartList {
    /// The rules in `xml` (a `SmartList` column), with each My Tag's path from `tag_path` (a
    /// `djmdMyTag` id to `Category/Tag`). `None` when it isn't such XML.
    pub fn parse(xml: &str, tag_path: impl Fn(&str) -> Option<String>) -> Option<SmartList> {
        let mut reader = quick_xml::Reader::from_str(xml);
        let mut any = None;
        let mut conditions = Vec::new();
        loop {
            let event = reader.read_event().ok()?;
            let e = match &event {
                quick_xml::events::Event::Empty(e) | quick_xml::events::Event::Start(e) => e,
                quick_xml::events::Event::Eof => break,
                _ => continue,
            };
            let attr = |name: &str| -> String {
                e.attributes()
                    .flatten()
                    .find(|a| a.key.as_ref() == name)
                    .and_then(|a| a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok().map(|v| v.into_owned()))
                    .unwrap_or_default()
            };
            match e.name().as_ref() {
                "NODE" if any.is_none() => any = Some(attr("LogicalOperator").trim() == "2"),
                "CONDITION" => {
                    let property = attr("PropertyName");
                    let mut left = attr("ValueLeft");
                    if property == "myTag" {
                        let id = left.trim().parse::<i64>().ok().map(|id| if id < 0 { id + (1 << 32) } else { id });
                        left = id.and_then(|id| tag_path(&id.to_string())).unwrap_or_default();
                    }
                    let operator = attr("Operator").trim().parse().unwrap_or(0);
                    conditions.push(Condition::new(
                        &property,
                        operator,
                        &attr("ValueUnit"),
                        &left,
                        &attr("ValueRight"),
                    ));
                }
                _ => {}
            }
        }
        Some(SmartList { any: any?, conditions })
    }

    /// The first condition that can't be evaluated, if any, or the list itself when it has none.
    pub fn unevaluable(&self) -> Option<String> {
        if self.conditions.is_empty() {
            return Some("it has no conditions".into());
        }
        self.conditions.iter().find(|c| c.rule.is_none()).map(|c| c.text.clone())
    }

    /// Whether `t` (whose My Tags are `tags`, as `Category/Tag`) is in the list on `today`. Only
    /// for a list [`SmartList::unevaluable`] passes: a condition it can't evaluate matches nothing.
    pub fn matches(&self, t: &Track, tags: &[String], today: NaiveDate) -> bool {
        let mut each = self.conditions.iter().map(|c| c.rule.as_ref().is_some_and(|r| r.matches(t, tags, today)));
        if self.any { each.any(|m| m) } else { each.all(|m| m) }
    }

    /// The list as a filter (see [`crate::query`]) that matches the same tracks, or the first
    /// condition a filter can't say.
    pub fn filter(&self) -> Result<String, String> {
        if let Some(why) = self.unevaluable() {
            return Err(why);
        }
        let terms: Vec<(Term, &Condition)> = self
            .conditions
            .iter()
            .map(|c| c.rule.as_ref().and_then(Rule::term).map(|t| (t, c)).ok_or_else(|| c.text.clone()))
            .collect::<Result<_, _>>()?;
        if !self.any || terms.len() == 1 {
            return Ok(terms.iter().map(|(t, _)| t.to_string()).collect::<Vec<_>>().join(" "));
        }
        // Any of several: only as one term's alternatives (`genre:"house|techno"`).
        let first = terms[0].0.clone();
        if let Some((_, c)) = terms.iter().find(|(t, _)| !t.either || t.field != first.field) {
            return Err(c.text.clone());
        }
        let values: Vec<String> = terms.into_iter().flat_map(|(t, _)| t.values).collect();
        Ok(Term { values, ..first }.to_string())
    }
}

/// One term of a filter: `field:"a|b"`, `-` in front when negated.
#[derive(Debug, Clone)]
struct Term {
    field: &'static str,
    negate: bool,
    values: Vec<String>,
    /// The values are text, written in quotes.
    quoted: bool,
    /// Its values can be alternatives (`a|b`): text it contains, a colour, a My Tag.
    either: bool,
}

impl std::fmt::Display for Term {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (q, minus) = (if self.quoted { "\"" } else { "" }, if self.negate { "-" } else { "" });
        write!(f, "{minus}{}:{q}{}{q}", self.field, self.values.join("|"))
    }
}

impl Condition {
    pub fn new(property: &str, operator: u8, unit: &str, left: &str, right: &str) -> Condition {
        let op = match operator {
            1 => "is",
            2 => "isn't",
            3 => "is greater than",
            4 => "is less than",
            5 => "is in the range",
            6 => "is in the last",
            7 => "isn't in the last",
            8 => "contains",
            9 => "doesn't contain",
            10 => "starts with",
            11 => "ends with",
            _ => "?",
        };
        let value = match operator {
            5 => format!("{left:?} to {right:?}"),
            6 | 7 => format!("{left} {unit}(s)"),
            _ => format!("{left:?}"),
        };
        Condition { text: format!("{property} {op} {value}"), rule: Rule::parse(property, operator, unit, left, right) }
    }
}

impl Rule {
    fn parse(property: &str, operator: u8, unit: &str, left: &str, right: &str) -> Option<Rule> {
        let text = |field| {
            let op = match operator {
                1 => TextOp::Is,
                2 => TextOp::IsNot,
                8 => TextOp::Contains,
                9 => TextOp::NotContains,
                10 => TextOp::StartsWith,
                11 => TextOp::EndsWith,
                _ => return None,
            };
            Some(Rule::Text { field, op, value: left.to_string() })
        };
        let number = |field, scale: fn(f64) -> f64| {
            let n = |s: &str| s.trim().parse::<f64>().ok().filter(|v| v.is_finite()).map(scale);
            Some(Rule::Number { field, op: compare(operator, n(left), || n(right))? })
        };
        let date = |field| {
            let op = match operator {
                6 | 7 => {
                    let unit = match unit.trim() {
                        "day" => Unit::Day,
                        "week" => Unit::Week,
                        "month" => Unit::Month,
                        "year" => Unit::Year,
                        _ => return None,
                    };
                    DateOp::InLast { n: left.trim().parse().ok()?, unit, within: operator == 6 }
                }
                // A whole date, so where it ends doesn't matter.
                _ => {
                    let d = |s: &str| date_value(s, false).filter(|_| s.trim().split(['-', '/']).count() == 3);
                    DateOp::Compare(compare(operator, d(left), || d(right))?)
                }
            };
            Some(Rule::Date { field, op })
        };
        let same = |v: f64| v;
        match property {
            "artist" => text(TextField::Artist),
            "album" => text(TextField::Album),
            "albumArtist" => text(TextField::AlbumArtist),
            "originalArtist" => text(TextField::OriginalArtist),
            "producer" => text(TextField::Producer),
            "genre" => text(TextField::Genre),
            "key" => text(TextField::Key),
            "label" => text(TextField::Label),
            "mixName" => text(TextField::MixName),
            "remixedBy" => text(TextField::Remixer),
            "name" => text(TextField::Title),
            "comments" => text(TextField::Comment),
            "fileName" => text(TextField::FileName),
            "bpm" => number(NumberField::Bpm, |v| if v.abs() >= 1000.0 { v / 100.0 } else { v }),
            "rating" => number(NumberField::Rating, same),
            "counter" => number(NumberField::Plays, same),
            "year" => number(NumberField::Year, same),
            "duration" => number(NumberField::Duration, same),
            "stockDate" => date(DateField::Added),
            "dateReleased" => date(DateField::Released),
            "grouping" if matches!(operator, 1 | 2) => {
                let id: u8 = left.trim().parse().ok()?;
                let color = match id {
                    0 => None,
                    id => Some(TrackColor::from_id(id)?),
                };
                Some(Rule::Color { equal: operator == 1, color })
            }
            "myTag" if matches!(operator, 8 | 9) => Some(Rule::Tag { has: operator == 8, path: left.to_string() }),
            _ => None,
        }
    }

    fn matches(&self, t: &Track, tags: &[String], today: NaiveDate) -> bool {
        match self {
            Rule::Text { field, op, value } => {
                let (hay, value) = (normalize(field.of(t)), normalize(value));
                match op {
                    TextOp::Is => hay == value,
                    TextOp::IsNot => hay != value,
                    TextOp::Contains => hay.contains(&value),
                    TextOp::NotContains => !hay.contains(&value),
                    TextOp::StartsWith => hay.starts_with(&value),
                    TextOp::EndsWith => hay.ends_with(&value),
                }
            }
            Rule::Number { field, op } => op.test(field.of(t)),
            Rule::Date { field, op } => {
                let date = field.of(t);
                match op {
                    DateOp::Compare(c) => match date_value(date, false) {
                        Some(v) => c.test(v),
                        None => matches!(c, Compare::IsNot(_)),
                    },
                    DateOp::InLast { n, unit, within } => {
                        let Some(since) = unit.before(today, *n) else { return !within };
                        let Some(d) = date.get(..10).and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()) else {
                            return false;
                        };
                        if *within { d > since } else { d < since }
                    }
                }
            }
            Rule::Color { equal, color } => (t.color == *color) == *equal,
            Rule::Tag { has, path } => {
                let path = normalize(path);
                let carried = !path.is_empty() && tags.iter().any(|tag| normalize(tag) == path);
                carried == *has
            }
        }
    }

    /// The filter term that matches what this does, if there is one.
    fn term(&self) -> Option<Term> {
        // Text a quoted filter value holds as it is: no quotes or `|`, and something to look for.
        let sayable = |v: &str| !v.contains(['"', '|']) && !normalize(v).is_empty();
        let term = |field, negate: bool, value: String, quoted, either: bool| {
            Some(Term { field, negate, values: vec![value], quoted, either: either && !negate })
        };
        match self {
            Rule::Text { field, op, value } => {
                let negate = match op {
                    TextOp::Contains => false,
                    TextOp::NotContains => true,
                    _ => return None,
                };
                let name = field.filter_name().filter(|_| sayable(value))?;
                term(name, negate, value.clone(), true, true)
            }
            Rule::Number { field, op } => {
                let name = match field {
                    NumberField::Bpm => "bpm",
                    NumberField::Rating => "rating",
                    NumberField::Plays => "plays",
                    NumberField::Year => "year",
                    NumberField::Duration => "time",
                };
                let (negate, value) = op.filter_value(|v| v.to_string());
                term(name, negate, value, false, false)
            }
            Rule::Date { field: DateField::Added, op: DateOp::Compare(c) } => {
                let day = |v: f64| {
                    let v = v as u32;
                    format!("{:04}-{:02}-{:02}", v / 10_000, v / 100 % 100, v % 100)
                };
                let (negate, value) = c.filter_value(day);
                term("added", negate, value, false, false)
            }
            Rule::Date { .. } => None,
            Rule::Color { equal, color } => {
                let name = color.map_or("none", |c| COLORS[usize::from(c.id()) - 1]);
                term("color", !equal, name.to_string(), false, true)
            }
            Rule::Tag { has, path } if sayable(path) && path.contains('/') => {
                term("tag", !has, path.clone(), true, true)
            }
            Rule::Tag { .. } => None,
        }
    }
}

/// A comparison from rekordbox's `Operator`, with its value (and a range's end).
fn compare<T: Copy>(operator: u8, left: Option<T>, right: impl FnOnce() -> Option<T>) -> Option<Compare<T>> {
    Some(match operator {
        1 => Compare::Is(left?),
        2 => Compare::IsNot(left?),
        3 => Compare::Greater(left?),
        4 => Compare::Less(left?),
        5 => Compare::Range(left?, right()?),
        _ => return None,
    })
}

impl Compare<f64> {
    fn test(self, v: f64) -> bool {
        match self {
            Compare::Is(x) => v == x,
            Compare::IsNot(x) => v != x,
            Compare::Greater(x) => v > x,
            Compare::Less(x) => v < x,
            Compare::Range(a, b) => v >= a.min(b) && v <= a.max(b),
        }
    }

    /// The value a filter term takes for this (`128..128`, `>5`, `1..3`), each number written by
    /// `show`, and whether the term is negated. A range is written low end first: a filter's
    /// date range from a later day to an earlier one holds nothing.
    fn filter_value(self, show: impl Fn(f64) -> String) -> (bool, String) {
        match self {
            Compare::Is(x) => (false, format!("{0}..{0}", show(x))),
            Compare::IsNot(x) => (true, format!("{0}..{0}", show(x))),
            Compare::Greater(x) => (false, format!(">{}", show(x))),
            Compare::Less(x) => (false, format!("<{}", show(x))),
            Compare::Range(a, b) => (false, format!("{}..{}", show(a.min(b)), show(a.max(b)))),
        }
    }
}

impl TextField {
    fn of(self, t: &Track) -> &str {
        match self {
            TextField::Artist => &t.artist,
            TextField::Album => &t.album,
            TextField::AlbumArtist => &t.album_artist,
            TextField::OriginalArtist => &t.original_artist,
            TextField::Producer => &t.composer,
            TextField::Genre => &t.genre,
            TextField::Key => &t.key,
            TextField::Label => &t.label,
            TextField::MixName => &t.mix_name,
            TextField::Remixer => &t.remixer,
            TextField::Title => &t.title,
            TextField::Comment => &t.comment,
            TextField::FileName => t.file_path.rsplit(['/', '\\']).next().unwrap_or_default(),
        }
    }

    /// The filter field matching just this one, if there is one.
    fn filter_name(self) -> Option<&'static str> {
        Some(match self {
            TextField::Album => "album",
            TextField::Genre => "genre",
            TextField::Label => "label",
            TextField::MixName => "mix",
            TextField::Remixer => "remixer",
            TextField::Title => "title",
            TextField::Comment => "comment",
            _ => return None,
        })
    }
}

impl NumberField {
    fn of(self, t: &Track) -> f64 {
        match self {
            NumberField::Bpm => t.bpm,
            NumberField::Rating => t.rating.into(),
            NumberField::Plays => t.play_count.into(),
            NumberField::Year => t.year.into(),
            NumberField::Duration => t.duration_secs.into(),
        }
    }
}

impl DateField {
    fn of(self, t: &Track) -> &str {
        match self {
            DateField::Added => &t.date_added,
            DateField::Released => &t.release_date,
        }
    }
}

impl Unit {
    /// `n` of these before `today`, if that's a date.
    fn before(self, today: NaiveDate, n: u32) -> Option<NaiveDate> {
        match self {
            Unit::Day => today.checked_sub_days(chrono::Days::new(n.into())),
            Unit::Week => today.checked_sub_days(chrono::Days::new(u64::from(n) * 7)),
            Unit::Month => today.checked_sub_months(Months::new(n)),
            Unit::Year => today.checked_sub_months(Months::new(n.checked_mul(12)?)),
        }
    }
}

#[cfg(test)]
mod tests;
