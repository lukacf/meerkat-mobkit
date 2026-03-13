//! Parameter parsing for scheduling RPC methods.

use super::*;

pub(super) fn parse_scheduling_params(
    params: &Value,
) -> Result<(Vec<ScheduleDefinition>, u64), String> {
    let object = params
        .as_object()
        .ok_or_else(|| "scheduling params must be a JSON object".to_string())?;
    let tick_ms = object
        .get("tick_ms")
        .and_then(Value::as_u64)
        .ok_or_else(|| "tick_ms must be a u64".to_string())?;
    if tick_ms > i64::MAX as u64 {
        return Err(format!("tick_ms must be <= {}", i64::MAX));
    }
    let schedules = object
        .get("schedules")
        .and_then(Value::as_array)
        .ok_or_else(|| "schedules must be an array".to_string())?;
    if schedules.len() > MAX_SCHEDULES_PER_REQUEST {
        return Err(format!(
            "schedules must contain at most {MAX_SCHEDULES_PER_REQUEST} entries"
        ));
    }

    let mut parsed = Vec::with_capacity(schedules.len());
    for schedule in schedules {
        let entry = schedule
            .as_object()
            .ok_or_else(|| "each schedule must be an object".to_string())?;
        let schedule_id = entry
            .get("schedule_id")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "schedule_id must be a non-empty string".to_string())?;
        let interval = entry
            .get("interval")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "interval must be a non-empty string".to_string())?;
        let timezone = entry
            .get("timezone")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "timezone must be a non-empty string".to_string())?;
        let jitter_ms = entry
            .get("jitter_ms")
            .map(|value| {
                value
                    .as_u64()
                    .ok_or_else(|| "jitter_ms must be a u64".to_string())
            })
            .transpose()?
            .unwrap_or(0);
        let catch_up = entry
            .get("catch_up")
            .map(|value| {
                value
                    .as_bool()
                    .ok_or_else(|| "catch_up must be a boolean".to_string())
            })
            .transpose()?
            .unwrap_or(false);
        let enabled = entry
            .get("enabled")
            .map(|value| {
                value
                    .as_bool()
                    .ok_or_else(|| "enabled must be a boolean".to_string())
            })
            .transpose()?
            .unwrap_or(true);
        parsed.push(ScheduleDefinition {
            schedule_id: schedule_id.trim().to_string(),
            interval: interval.to_string(),
            timezone: timezone.to_string(),
            enabled,
            jitter_ms,
            catch_up,
        });
    }

    validate_schedules(&parsed).map_err(format_schedule_validation_error)?;

    Ok((parsed, tick_ms))
}

pub(super) fn format_schedule_validation_error(err: ScheduleValidationError) -> String {
    match err {
        ScheduleValidationError::EmptyScheduleId => {
            "schedule_id must be a non-empty string".to_string()
        }
        ScheduleValidationError::DuplicateScheduleId(schedule_id) => {
            format!("duplicate schedule_id '{schedule_id}' is not allowed")
        }
        ScheduleValidationError::InvalidTickMs(tick_ms) => {
            format!(
                "tick_ms '{tick_ms}' is unsupported (must be <= {})",
                i64::MAX
            )
        }
        ScheduleValidationError::InvalidInterval {
            schedule_id,
            interval,
        } => format!("invalid interval '{interval}' for schedule_id '{schedule_id}'"),
        ScheduleValidationError::InvalidTimezone {
            schedule_id,
            timezone,
        } => format!("invalid timezone '{timezone}' for schedule_id '{schedule_id}'"),
    }
}
