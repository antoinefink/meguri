//! Questions answered from the parse without rendering
//! (`specs/render/template-syntax.md` T11): referenced names, whether a
//! template would render something for a set of non-empty names, and the
//! legacy card requirement stored with a note type. Card generation (U27),
//! note type validation (U29, R942) and the template editor (U200) use them.

use crate::parser::{NodeKind, ParsedTemplate};
use crate::render::is_card_number;

/// The two forms of T11.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// Blank check (T10) and card generation.
    Strict,
    /// Legacy requirements (T11.3) only: a negated section's contents count
    /// whatever its key.
    Legacy,
}

/// The kind of a legacy card requirement (T11.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequirementKind {
    Any,
    All,
    None,
}

impl RequirementKind {
    /// The name stored in the note type (`specs/model/schema11-json.md`
    /// S4.1).
    pub const fn name(self) -> &'static str {
        match self {
            RequirementKind::Any => "any",
            RequirementKind::All => "all",
            RequirementKind::None => "none",
        }
    }
}

/// A template's legacy card requirement: its kind and field ordinals in
/// ascending order (none for [`RequirementKind::None`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyRequirement {
    pub kind: RequirementKind,
    pub ords: Vec<usize>,
}

impl ParsedTemplate {
    /// T11.1: every replacement key and every section key that is not a
    /// card number (`c` and a u32), outside comments, sections included;
    /// each once, in order of first appearance. Special names, `FrontSide`,
    /// the empty key and names of no field are kept: callers decide.
    pub fn field_references(&self) -> Vec<&str> {
        let keys = self.nodes().iter().filter_map(|node| match &node.kind {
            NodeKind::Replacement { key, .. } => Some(key.as_str()),
            NodeKind::Open { key, .. } if !is_card_number(key) => Some(key.as_str()),
            _ => None,
        });
        first_appearances(keys)
    }

    /// T11.1: the keys of replacements whose filter names include `cloze`.
    pub fn cloze_references(&self) -> Vec<&str> {
        let keys = self.nodes().iter().filter_map(|node| match &node.kind {
            NodeKind::Replacement { key, filters } if filters.iter().any(|f| f == "cloze") => Some(key.as_str()),
            _ => None,
        });
        first_appearances(keys)
    }

    /// T11.2: whether a replacement of a non-empty name would be met with
    /// every enclosing section passing (a conditional section on a
    /// non-empty name; a negated one on an empty name, or any negated one in
    /// the legacy form). Filters do not matter. A template with a
    /// structural error (T6) renders an error, so nothing.
    pub fn renders_something(&self, nonempty: impl Fn(&str) -> bool, form: Form) -> bool {
        if self.structure_error().is_some() {
            return false;
        }
        let mut passes: Vec<bool> = Vec::new();
        let mut failing = 0usize;
        for node in self.nodes() {
            match &node.kind {
                NodeKind::Replacement { key, .. } if failing == 0 && nonempty(key) => return true,
                NodeKind::Open { key, negated } => {
                    let pass = match (negated, form) {
                        (false, _) => nonempty(key),
                        (true, Form::Strict) => !nonempty(key),
                        (true, Form::Legacy) => true,
                    };
                    passes.push(pass);
                    failing += usize::from(!pass);
                }
                NodeKind::Close { .. } => failing -= usize::from(passes.pop() == Some(false)),
                _ => {}
            }
        }
        false
    }
}

/// T11.3: the requirement Anki stores for a question template, from the
/// legacy form of T11.2 over the note type's own field names (in ordinal
/// order) only.
pub fn legacy_requirement(question: &ParsedTemplate, field_names: &[&str]) -> LegacyRequirement {
    let none = LegacyRequirement {
        kind: RequirementKind::None,
        ords: Vec::new(),
    };
    if question.structure_error().is_some() {
        return none;
    }
    let renders_with = |nonempty: &dyn Fn(usize) -> bool| {
        question.renders_something(
            |name| field_names.iter().position(|f| *f == name).is_some_and(nonempty),
            Form::Legacy,
        )
    };
    let any: Vec<usize> = (0..field_names.len())
        .filter(|&only| renders_with(&|ord| ord == only))
        .collect();
    if !any.is_empty() {
        return LegacyRequirement {
            kind: RequirementKind::Any,
            ords: any,
        };
    }
    let all: Vec<usize> = (0..field_names.len())
        .filter(|&removed| !renders_with(&|ord| ord != removed))
        .collect();
    if !all.is_empty() && renders_with(&|_| true) {
        return LegacyRequirement {
            kind: RequirementKind::All,
            ords: all,
        };
    }
    none
}

fn first_appearances<'a>(keys: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let mut seen = std::collections::HashSet::new();
    keys.filter(|key| seen.insert(*key)).collect()
}
