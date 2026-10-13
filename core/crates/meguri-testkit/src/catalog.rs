//! The harvest catalog (`conformance/catalog/harvest.csv`) and its coverage
//! report (dsl.md D14). The catalog never names Anki files or tests; that
//! link lives on the spec side (`oracle/harvest/links.csv`), which this crate
//! never reads.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use serde_json::{Value, json};

use crate::exec::Executor;
use crate::suite::{Status, Suite};

#[derive(Debug, Clone)]
pub struct CatalogError(String);

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CatalogError {}

pub const HEADER: [&str; 7] = [
    "harvest_id",
    "language",
    "area",
    "classification",
    "reason",
    "scenarios",
    "status",
];
pub const LANGUAGES: [&str; 3] = ["rust", "python", "typescript"];
pub const CLASSIFICATIONS: [&str; 5] = [
    "in-scope",
    "preservation-only",
    "out-of-scope",
    "not-applicable",
    "unclassified",
];
pub const STATUSES: [&str; 3] = ["open", "deferred", "done"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarvestRow {
    pub harvest_id: String,
    pub language: String,
    pub area: String,
    pub classification: String,
    pub reason: String,
    pub scenarios: Vec<String>,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct Catalog {
    pub rows: Vec<HarvestRow>,
}

impl Catalog {
    pub fn read(path: &Path) -> Result<Catalog, CatalogError> {
        let text =
            std::fs::read_to_string(path).map_err(|e| CatalogError(format!("reading {}: {e}", path.display())))?;
        Catalog::parse(&text)
    }

    /// Parses and checks the row rules of D14.
    pub fn parse(text: &str) -> Result<Catalog, CatalogError> {
        let records = csv_records(text)?;
        let Some((header, body)) = records.split_first() else {
            return Err(CatalogError("the catalog is empty; it needs the header row".to_owned()));
        };
        if header.iter().map(String::as_str).ne(HEADER) {
            return Err(CatalogError(format!("the header must be {}", HEADER.join(","))));
        }
        let mut seen = BTreeSet::new();
        let mut rows = Vec::with_capacity(body.len());
        for (i, record) in body.iter().enumerate() {
            let line = i + 2;
            let bad = |why: String| CatalogError(format!("line {line}: {why}"));
            let [id, language, area, classification, reason, scenarios, status] = record.as_slice() else {
                return Err(bad(format!("{} fields instead of {}", record.len(), HEADER.len())));
            };
            if !(id.len() == 13
                && id.starts_with('h')
                && id[1..]
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
            {
                return Err(bad(format!("harvest_id {id:?} must be h and 12 lower-case hex digits")));
            }
            if !seen.insert(id.clone()) {
                return Err(bad(format!("harvest_id {id} appears twice")));
            }
            if !LANGUAGES.contains(&language.as_str()) {
                return Err(bad(format!(
                    "language {language:?} is not one of {}",
                    LANGUAGES.join(", ")
                )));
            }
            if area.is_empty() || !area.bytes().all(|b| b.is_ascii_lowercase() || b == b'-') {
                return Err(bad(format!("area {area:?} must be a lower-case area name")));
            }
            if !CLASSIFICATIONS.contains(&classification.as_str()) {
                return Err(bad(format!(
                    "classification {classification:?} is not one of {}",
                    CLASSIFICATIONS.join(", ")
                )));
            }
            if !STATUSES.contains(&status.as_str()) {
                return Err(bad(format!("status {status:?} is not one of {}", STATUSES.join(", "))));
            }
            let unclassified = classification == "unclassified";
            if unclassified && status != "open" {
                return Err(bad("an unclassified row stays open".to_owned()));
            }
            if !unclassified && reason.trim().is_empty() {
                return Err(bad("a classified row needs a one-line reason".to_owned()));
            }
            let scenarios: Vec<String> = scenarios.split_whitespace().map(str::to_owned).collect();
            let needs_scenarios = matches!(classification.as_str(), "in-scope" | "preservation-only");
            if status == "done" && needs_scenarios && scenarios.is_empty() {
                return Err(bad(format!(
                    "status done needs linked scenarios for a {classification} row"
                )));
            }
            rows.push(HarvestRow {
                harvest_id: id.clone(),
                language: language.clone(),
                area: area.clone(),
                classification: classification.clone(),
                reason: reason.clone(),
                scenarios,
                status: status.clone(),
            });
        }
        Ok(Catalog { rows })
    }
}

/// RFC 4180 records: commas, double-quoted fields with `""` escapes, LF or
/// CRLF line ends.
fn csv_records(text: &str) -> Result<Vec<Vec<String>>, CatalogError> {
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut chars = text.chars().peekable();
    let mut quoted = false;
    let mut line = 1;
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                c => {
                    if c == '\n' {
                        line += 1;
                    }
                    field.push(c);
                }
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => quoted = true,
            ',' => record.push(std::mem::take(&mut field)),
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' => {
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
                line += 1;
            }
            '"' => return Err(CatalogError(format!("line {line}: a quote inside an unquoted field"))),
            c => field.push(c),
        }
    }
    if quoted {
        return Err(CatalogError(format!("line {line}: an unterminated quoted field")));
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        records.push(record);
    }
    Ok(records)
}

#[derive(Debug, Clone, Default)]
pub struct ClassSummary {
    pub rows: usize,
    /// Row statuses: the worst status of its linked scenarios, or `unlinked`.
    pub statuses: BTreeMap<String, usize>,
}

#[derive(Debug, Clone)]
pub struct RowReport {
    pub harvest_id: String,
    pub language: String,
    pub area: String,
    pub classification: String,
    pub status: String,
    pub scenario_statuses: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub struct HarvestReport {
    pub anki_version: String,
    pub rows_total: usize,
    pub by_language: BTreeMap<String, BTreeMap<String, usize>>,
    pub by_classification: BTreeMap<String, ClassSummary>,
    pub rows: Vec<RowReport>,
    pub problems: Vec<String>,
}

/// Worst first: a row's status is the first of these among its scenarios.
const SEVERITY: [Status; 8] = [
    Status::Invalid,
    Status::Error,
    Status::Fail,
    Status::Stale,
    Status::Missing,
    Status::Pending,
    Status::Live,
    Status::Pass,
];

/// Counts rows per language and classification, and the T0 status (with this
/// executor) of every linked scenario.
pub fn harvest_report(catalog: &Catalog, suite: &Suite, executor: &dyn Executor) -> HarvestReport {
    let mut report = HarvestReport {
        anki_version: suite.pin.anki_version.clone(),
        rows_total: catalog.rows.len(),
        ..HarvestReport::default()
    };
    for language in LANGUAGES {
        report.by_language.insert(
            language.to_owned(),
            CLASSIFICATIONS.iter().map(|c| ((*c).to_owned(), 0)).collect(),
        );
    }
    for class in CLASSIFICATIONS {
        report
            .by_classification
            .insert(class.to_owned(), ClassSummary::default());
    }
    let files = suite.discover(&[]).unwrap_or_default();
    let mut cache: BTreeMap<String, Option<Status>> = BTreeMap::new();
    for row in &catalog.rows {
        let mut scenario_statuses = BTreeMap::new();
        for id in &row.scenarios {
            let status = *cache.entry(id.clone()).or_insert_with(|| {
                files
                    .iter()
                    .find(|f| &f.rel == id)
                    .map(|f| suite.check(f, executor).status)
            });
            match status {
                Some(status) => {
                    scenario_statuses.insert(id.clone(), status.as_str().to_owned());
                }
                None => report
                    .problems
                    .push(format!("{}: linked scenario {id} does not exist", row.harvest_id)),
            }
        }
        let status = SEVERITY
            .iter()
            .find(|s| scenario_statuses.values().any(|v| v == s.as_str()))
            .map_or("unlinked", |s| s.as_str())
            .to_owned();
        *report
            .by_language
            .entry(row.language.clone())
            .or_default()
            .entry(row.classification.clone())
            .or_default() += 1;
        let summary = report.by_classification.entry(row.classification.clone()).or_default();
        summary.rows += 1;
        *summary.statuses.entry(status.clone()).or_default() += 1;
        report.rows.push(RowReport {
            harvest_id: row.harvest_id.clone(),
            language: row.language.clone(),
            area: row.area.clone(),
            classification: row.classification.clone(),
            status,
            scenario_statuses,
        });
    }
    report
}

const ROW_STATUSES: [&str; 9] = [
    "pass", "fail", "error", "pending", "stale", "missing", "invalid", "live", "unlinked",
];

impl HarvestReport {
    pub fn to_json(&self) -> Value {
        json!({
            "anki_version": self.anki_version,
            "rows_total": self.rows_total,
            "by_language": self.by_language,
            "by_classification": self.by_classification.iter().map(|(class, s)| {
                (class.clone(), json!({"rows": s.rows, "statuses": s.statuses}))
            }).collect::<serde_json::Map<String, Value>>(),
            "problems": self.problems,
            "rows": self.rows.iter().map(|r| json!({
                "harvest_id": r.harvest_id,
                "language": r.language,
                "area": r.area,
                "classification": r.classification,
                "status": r.status,
                "scenario_statuses": r.scenario_statuses,
            })).collect::<Vec<_>>(),
        })
    }

    pub fn render_markdown(&self) -> String {
        let mut md = String::from("# Harvest coverage\n\n");
        let per_language: Vec<String> = self
            .by_language
            .iter()
            .map(|(language, classes)| format!("{language} {}", classes.values().sum::<usize>()))
            .collect();
        md.push_str(&format!(
            "Anki tests inventoried at the pinned release (anki {}): {} ({}).\n\n",
            self.anki_version,
            self.rows_total,
            per_language.join(", ")
        ));
        md.push_str("## By classification\n\nRow status: the worst T0 status of its linked scenarios.\n\n");
        md.push_str(&format!("| Classification | Rows | {} |\n", ROW_STATUSES.join(" | ")));
        md.push_str(&format!("|---|---|{}\n", "---|".repeat(ROW_STATUSES.len())));
        for (class, summary) in &self.by_classification {
            let cells: Vec<String> = ROW_STATUSES
                .iter()
                .map(|s| summary.statuses.get(*s).copied().unwrap_or(0).to_string())
                .collect();
            md.push_str(&format!("| {class} | {} | {} |\n", summary.rows, cells.join(" | ")));
        }
        md.push_str("\n## By language\n\n");
        md.push_str(&format!("| Language | {} |\n", CLASSIFICATIONS.join(" | ")));
        md.push_str(&format!("|---|{}\n", "---|".repeat(CLASSIFICATIONS.len())));
        for (language, classes) in &self.by_language {
            let cells: Vec<String> = CLASSIFICATIONS
                .iter()
                .map(|c| classes.get(*c).copied().unwrap_or(0).to_string())
                .collect();
            md.push_str(&format!("| {language} | {} |\n", cells.join(" | ")));
        }
        if !self.problems.is_empty() {
            md.push_str("\n## Problems\n\n");
            for p in &self.problems {
                md.push_str(&format!("- {p}\n"));
            }
        }
        md.push_str("\n## Rows\n\n| Harvest id | Language | Area | Classification | Status | Scenarios |\n|---|---|---|---|---|---|\n");
        for r in &self.rows {
            let scenarios: Vec<String> = r
                .scenario_statuses
                .iter()
                .map(|(id, s)| format!("{id} ({s})"))
                .collect();
            md.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} |\n",
                r.harvest_id,
                r.language,
                r.area,
                r.classification,
                r.status,
                scenarios.join(", ")
            ));
        }
        md
    }
}
