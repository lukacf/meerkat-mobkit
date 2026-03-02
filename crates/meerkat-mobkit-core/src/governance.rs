use std::fmt;

pub const STRICT_TRACEABILITY_STATUSES: &[&str] = &[
    "TYPED",
    "WIRED",
    "VALIDATED",
    "PROVISIONAL",
    "MISSING",
    "DEFERRED",
    "STUBBED",
];
const REQUIRED_GOVERNANCE_STATE: &str = "realignment_in_progress";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GovernanceValidationError {
    MissingGovernanceState { file: String },
    InvalidGovernanceState { file: String, found: String },
    NoTraceabilityRows,
    InvalidTraceabilityStatus { line: usize, status: String },
    MissingTraceabilityEvidence { line: usize },
    InvalidTraceabilityRow { line: usize },
}

impl fmt::Display for GovernanceValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingGovernanceState { file } => {
                write!(f, "missing governance_state in {file}")
            }
            Self::InvalidGovernanceState { file, found } => write!(
                f,
                "invalid governance_state in {file}: expected {REQUIRED_GOVERNANCE_STATE}, found {found}"
            ),
            Self::NoTraceabilityRows => write!(f, "no traceability rows found"),
            Self::InvalidTraceabilityStatus { line, status } => write!(
                f,
                "invalid traceability status at line {line}: {status}"
            ),
            Self::MissingTraceabilityEvidence { line } => {
                write!(f, "missing traceability evidence/link at line {line}")
            }
            Self::InvalidTraceabilityRow { line } => {
                write!(f, "invalid traceability row format at line {line}")
            }
        }
    }
}

impl std::error::Error for GovernanceValidationError {}

pub fn validate_governance_state(
    file_name: &str,
    content: &str,
) -> Result<(), GovernanceValidationError> {
    let line = content
        .lines()
        .find(|line| line.trim_start().starts_with("governance_state:"))
        .ok_or_else(|| GovernanceValidationError::MissingGovernanceState {
            file: file_name.to_string(),
        })?;

    let found = line
        .split_once(':')
        .map(|(_, value)| value.trim())
        .unwrap_or_default()
        .to_string();

    if found != REQUIRED_GOVERNANCE_STATE {
        return Err(GovernanceValidationError::InvalidGovernanceState {
            file: file_name.to_string(),
            found,
        });
    }

    Ok(())
}

pub fn validate_traceability_statuses(markdown: &str) -> Result<(), GovernanceValidationError> {
    let mut seen_rows = false;
    let mut status_column = None;
    let mut evidence_or_link_column = None;
    let mut header_line = None;

    for (idx, line) in markdown.lines().enumerate() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            continue;
        }

        let columns = trimmed
            .trim_start_matches('|')
            .trim_end_matches('|')
            .split('|')
            .map(|part| part.trim())
            .collect::<Vec<_>>();

        if columns.is_empty()
            || columns
                .iter()
                .all(|column| !column.is_empty() && column.chars().all(|ch| ch == '-' || ch == ':'))
        {
            continue;
        }

        if status_column.is_none() {
            header_line = Some(idx + 1);
            status_column = columns
                .iter()
                .position(|column| column.eq_ignore_ascii_case("Status"));
            evidence_or_link_column = columns
                .iter()
                .position(|column| is_evidence_or_link_column(column));
            continue;
        }

        let Some(status_column) = status_column else {
            continue;
        };
        let Some(evidence_or_link_column) = evidence_or_link_column else {
            return Err(GovernanceValidationError::InvalidTraceabilityRow {
                line: header_line.unwrap_or(idx + 1),
            });
        };
        if columns.len() <= status_column || columns.len() <= evidence_or_link_column {
            return Err(GovernanceValidationError::InvalidTraceabilityRow { line: idx + 1 });
        }

        seen_rows = true;
        let evidence = columns[evidence_or_link_column].trim_matches('`').trim();
        if is_missing_evidence(evidence) {
            return Err(GovernanceValidationError::MissingTraceabilityEvidence { line: idx + 1 });
        }
        let status = columns[status_column].trim_matches('`');
        if !STRICT_TRACEABILITY_STATUSES.contains(&status) {
            return Err(GovernanceValidationError::InvalidTraceabilityStatus {
                line: idx + 1,
                status: status.to_string(),
            });
        }
    }

    if !seen_rows {
        return Err(GovernanceValidationError::NoTraceabilityRows);
    }

    Ok(())
}

fn is_evidence_or_link_column(column: &str) -> bool {
    let normalized = column
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    normalized.contains("evidence") || normalized.contains("link")
}

fn is_missing_evidence(value: &str) -> bool {
    if value.is_empty() {
        return true;
    }

    let normalized = value.to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "-" | "--" | "n/a" | "na" | "none" | "null" | "todo" | "tbd" | "placeholder"
    )
}

pub fn validate_phase0_governance_contracts(
    spec_yaml: &str,
    plan_yaml: &str,
    checklist_yaml: &str,
    traceability_markdown: &str,
) -> Result<(), GovernanceValidationError> {
    validate_governance_state(".rct/spec.yaml", spec_yaml)?;
    validate_governance_state(".rct/plan.yaml", plan_yaml)?;
    validate_governance_state(".rct/checklist.yaml", checklist_yaml)?;
    validate_traceability_statuses(traceability_markdown)?;
    Ok(())
}
