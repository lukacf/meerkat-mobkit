use std::fmt;

pub const STRICT_TRACEABILITY_STATUSES: &[&str] = &["MISSING", "TYPED_ONLY", "WIRED"];
const REQUIRED_GOVERNANCE_STATE: &str = "realignment_in_progress";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GovernanceValidationError {
    MissingGovernanceState { file: String },
    InvalidGovernanceState { file: String, found: String },
    NoTraceabilityRows,
    InvalidTraceabilityStatus { line: usize, status: String },
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

    for (idx, line) in markdown.lines().enumerate() {
        let trimmed = line.trim();
        if !trimmed.starts_with("| TR-") {
            continue;
        }

        seen_rows = true;
        let columns = trimmed
            .split('|')
            .map(|part| part.trim())
            .collect::<Vec<_>>();

        // Rows are expected as:
        // | Trace-ID | Requirement ID | Phase | Evidence Log | Status |
        // With leading/trailing separators split() yields 7 entries.
        if columns.len() < 7 {
            return Err(GovernanceValidationError::InvalidTraceabilityRow { line: idx + 1 });
        }

        let status = columns[5];
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
