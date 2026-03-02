use super::module_boundary::{
    call_module_mcp_tool_json, mcp_required_error, module_uses_mcp, CORE_MODULE_MCP_TIMEOUT,
    SCHEDULING_DISPATCH_MCP_TOOL,
};
use super::*;

pub fn evaluate_schedules_at_tick(
    schedules: &[ScheduleDefinition],
    tick_ms: u64,
) -> Result<ScheduleEvaluation, ScheduleValidationError> {
    validate_schedule_tick_ms_supported(tick_ms)?;
    validate_schedules(schedules)?;
    let mut due_triggers = schedules
        .iter()
        .filter(|schedule| schedule.enabled)
        .filter_map(|schedule| {
            let canonical_schedule_id = canonical_schedule_id(&schedule.schedule_id);
            let interval =
                parse_schedule_interval(&schedule.interval).expect("validated schedule interval");
            let timezone =
                parse_schedule_timezone(&schedule.timezone).expect("validated schedule timezone");
            let due_tick_ms = latest_due_tick_at_or_before(
                &canonical_schedule_id,
                &interval,
                &timezone,
                schedule.jitter_ms,
                tick_ms,
            )?;
            if due_tick_ms != tick_ms {
                return None;
            }
            Some(ScheduleTrigger {
                schedule_id: canonical_schedule_id,
                interval: schedule.interval.clone(),
                timezone: schedule.timezone.clone(),
                due_tick_ms,
            })
        })
        .collect::<Vec<_>>();

    due_triggers.sort_by(|left, right| {
        left.due_tick_ms
            .cmp(&right.due_tick_ms)
            .then_with(|| left.schedule_id.cmp(&right.schedule_id))
            .then_with(|| left.interval.cmp(&right.interval))
            .then_with(|| left.timezone.cmp(&right.timezone))
    });

    Ok(ScheduleEvaluation {
        tick_ms,
        due_triggers,
    })
}

pub(crate) fn validate_schedules(
    schedules: &[ScheduleDefinition],
) -> Result<(), ScheduleValidationError> {
    let mut seen = BTreeSet::new();
    for schedule in schedules {
        let canonical_schedule_id = canonical_schedule_id(&schedule.schedule_id);
        if canonical_schedule_id.is_empty() {
            return Err(ScheduleValidationError::EmptyScheduleId);
        }
        if !seen.insert(canonical_schedule_id.clone()) {
            return Err(ScheduleValidationError::DuplicateScheduleId(
                canonical_schedule_id,
            ));
        }
        if parse_schedule_interval(&schedule.interval).is_none() {
            return Err(ScheduleValidationError::InvalidInterval {
                schedule_id: canonical_schedule_id.clone(),
                interval: schedule.interval.clone(),
            });
        }
        if parse_schedule_timezone(&schedule.timezone).is_none() {
            return Err(ScheduleValidationError::InvalidTimezone {
                schedule_id: canonical_schedule_id,
                timezone: schedule.timezone.clone(),
            });
        }
    }
    Ok(())
}

impl MobkitRuntimeHandle {
    fn parse_scheduling_runtime_injection_response(
        response: Value,
    ) -> Result<Option<(String, String)>, RuntimeBoundaryError> {
        let Some(injection) = response
            .as_object()
            .and_then(|payload| payload.get("runtime_injection"))
            .and_then(Value::as_object)
            .cloned()
        else {
            return Ok(None);
        };
        let member_id = injection
            .get("member_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                RuntimeBoundaryError::Mcp(McpBoundaryError::InvalidToolPayload {
                    module_id: "scheduling".to_string(),
                    tool: SCHEDULING_DISPATCH_MCP_TOOL.to_string(),
                    reason: "runtime_injection.member_id must be a non-empty string".to_string(),
                })
            })?;
        let message = injection
            .get("message")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                RuntimeBoundaryError::Mcp(McpBoundaryError::InvalidToolPayload {
                    module_id: "scheduling".to_string(),
                    tool: SCHEDULING_DISPATCH_MCP_TOOL.to_string(),
                    reason: "runtime_injection.message must be a non-empty string".to_string(),
                })
            })?;
        Ok(Some((member_id.to_string(), message.to_string())))
    }

    fn scheduling_runtime_injection_for_dispatch(
        &self,
        schedule_id: &str,
        interval: &str,
        timezone: &str,
        due_tick_ms: u64,
        tick_ms: u64,
        claim_key: &str,
    ) -> Result<Option<(String, String)>, RuntimeBoundaryError> {
        let Some((scheduling_module, pre_spawn)) = self.module_and_prespawn("scheduling") else {
            return Ok(None);
        };
        if !self.is_module_loaded("scheduling") {
            return Ok(None);
        }
        if !module_uses_mcp(scheduling_module, pre_spawn) {
            return Err(mcp_required_error(
                "scheduling",
                SCHEDULING_DISPATCH_MCP_TOOL,
            ));
        }
        let response = call_module_mcp_tool_json(
            scheduling_module,
            pre_spawn,
            SCHEDULING_DISPATCH_MCP_TOOL,
            &serde_json::json!({
                "schedule_id": schedule_id,
                "interval": interval,
                "timezone": timezone,
                "due_tick_ms": due_tick_ms,
                "tick_ms": tick_ms,
                "claim_key": claim_key,
            }),
            CORE_MODULE_MCP_TIMEOUT,
        )?;
        Self::parse_scheduling_runtime_injection_response(response)
    }

    fn next_scheduling_dispatch_sequence(&mut self) -> u64 {
        let sequence = self.scheduling_dispatch_sequence;
        self.scheduling_dispatch_sequence = self.scheduling_dispatch_sequence.saturating_add(1);
        sequence
    }
    pub fn evaluate_schedule_tick(
        &self,
        schedules: &[ScheduleDefinition],
        tick_ms: u64,
    ) -> Result<ScheduleEvaluation, ScheduleValidationError> {
        evaluate_schedules_at_tick(schedules, tick_ms)
    }

    pub fn dispatch_schedule_tick(
        &mut self,
        schedules: &[ScheduleDefinition],
        tick_ms: u64,
    ) -> Result<ScheduleDispatchReport, ScheduleValidationError> {
        validate_schedule_tick_ms_supported(tick_ms)?;
        validate_schedules(schedules)?;
        self.prune_schedule_claims(tick_ms);
        self.prune_scheduling_last_due_ticks(tick_ms);
        let mut due_triggers = schedules
            .iter()
            .filter(|schedule| schedule.enabled)
            .filter_map(|schedule| {
                let canonical_schedule_id = canonical_schedule_id(&schedule.schedule_id);
                let interval = parse_schedule_interval(&schedule.interval)
                    .expect("validated schedule interval");
                let timezone = parse_schedule_timezone(&schedule.timezone)
                    .expect("validated schedule timezone");
                let due_tick_ms = latest_due_tick_at_or_before(
                    &canonical_schedule_id,
                    &interval,
                    &timezone,
                    schedule.jitter_ms,
                    tick_ms,
                )?;
                let last_due_tick = self
                    .scheduling_last_due_ticks
                    .get(&canonical_schedule_id)
                    .copied();
                if schedule.catch_up {
                    if last_due_tick.is_some_and(|last| last >= due_tick_ms) {
                        return None;
                    }
                } else if last_due_tick
                    .is_some_and(|last| last >= due_tick_ms && due_tick_ms != tick_ms)
                {
                    return None;
                }
                Some((schedule, canonical_schedule_id, due_tick_ms))
            })
            .collect::<Vec<_>>();
        due_triggers.sort_by(
            |(left_schedule, left_schedule_id, left_due_tick),
             (right_schedule, right_schedule_id, right_due_tick)| {
                left_due_tick
                    .cmp(right_due_tick)
                    .then_with(|| left_schedule_id.cmp(right_schedule_id))
                    .then_with(|| left_schedule.interval.cmp(&right_schedule.interval))
                    .then_with(|| left_schedule.timezone.cmp(&right_schedule.timezone))
            },
        );
        let mut dispatched = Vec::new();
        let mut skipped_claims = Vec::new();
        let scheduling_signal = self.scheduling_supervisor_signal();
        let mut supervisor_restart_emitted = false;

        for (trigger, canonical_schedule_id, due_tick_ms) in due_triggers.iter() {
            let claim_key = format!("{canonical_schedule_id}:{due_tick_ms}");
            if !self.record_schedule_claim(claim_key.clone(), tick_ms) {
                skipped_claims.push(claim_key);
                continue;
            }
            self.scheduling_last_due_ticks
                .insert(canonical_schedule_id.clone(), *due_tick_ms);
            self.prune_scheduling_last_due_ticks(tick_ms);

            let event_sequence = self.next_scheduling_dispatch_sequence();
            let event_id = format!(
                "evt-schedule-{}-{due_tick_ms}-{event_sequence}",
                canonical_schedule_id
            );
            insert_event_sorted(
                &mut self.merged_events,
                EventEnvelope {
                    event_id: event_id.clone(),
                    source: "module".to_string(),
                    timestamp_ms: tick_ms,
                    event: UnifiedEvent::Module(ModuleEvent {
                        module: "scheduling".to_string(),
                        event_type: "dispatch".to_string(),
                        payload: serde_json::json!({
                            "schedule_id": canonical_schedule_id,
                            "interval": trigger.interval,
                            "timezone": trigger.timezone,
                            "tick_ms": tick_ms,
                            "due_tick_ms": due_tick_ms,
                            "claim_key": claim_key,
                            "supervisor_signal": scheduling_signal,
                        }),
                    }),
                },
            );

            if let Some(signal) = &scheduling_signal {
                if signal.restart_observed && !supervisor_restart_emitted {
                    insert_event_sorted(
                        &mut self.merged_events,
                        EventEnvelope {
                            event_id: format!(
                                "evt-scheduling-supervisor-{tick_ms}-{event_sequence}",
                            ),
                            source: "module".to_string(),
                            timestamp_ms: tick_ms,
                            event: UnifiedEvent::Module(ModuleEvent {
                                module: "scheduling".to_string(),
                                event_type: "supervisor.restart".to_string(),
                                payload: serde_json::json!({
                                    "module_id": signal.module_id,
                                    "latest_state": signal.latest_state,
                                    "latest_attempt": signal.latest_attempt,
                                    "restart_observed": signal.restart_observed,
                                }),
                            }),
                        },
                    );
                    supervisor_restart_emitted = true;
                }
            }

            let mut runtime_injection = None;
            let mut runtime_injection_error = None;
            match self.scheduling_runtime_injection_for_dispatch(
                canonical_schedule_id,
                &trigger.interval,
                &trigger.timezone,
                *due_tick_ms,
                tick_ms,
                &claim_key,
            ) {
                Ok(Some((member_id, message))) => {
                    let injection_event_id =
                        format!("evt-runtime-injection-{tick_ms}-{event_sequence}");
                    insert_event_sorted(
                        &mut self.merged_events,
                        EventEnvelope {
                            event_id: injection_event_id.clone(),
                            source: "module".to_string(),
                            timestamp_ms: tick_ms,
                            event: UnifiedEvent::Module(ModuleEvent {
                                module: "runtime".to_string(),
                                event_type: "injection.dispatch".to_string(),
                                payload: serde_json::json!({
                                    "schedule_id": canonical_schedule_id,
                                    "claim_key": claim_key,
                                    "member_id": member_id,
                                    "message": message,
                                }),
                            }),
                        },
                    );
                    runtime_injection = Some(ScheduleRuntimeInjection {
                        member_id,
                        message,
                        injection_event_id,
                    });
                }
                Ok(None) => {}
                Err(error) => {
                    runtime_injection_error = Some(format!("{error:?}"));
                    insert_event_sorted(
                        &mut self.merged_events,
                        EventEnvelope {
                            event_id: format!(
                                "evt-runtime-injection-failed-{tick_ms}-{event_sequence}"
                            ),
                            source: "module".to_string(),
                            timestamp_ms: tick_ms,
                            event: UnifiedEvent::Module(ModuleEvent {
                                module: "runtime".to_string(),
                                event_type: "runtime.injection.failed".to_string(),
                                payload: serde_json::json!({
                                    "schedule_id": canonical_schedule_id,
                                    "claim_key": claim_key,
                                    "error": format!("{error:?}"),
                                }),
                            }),
                        },
                    );
                }
            }

            dispatched.push(ScheduleDispatch {
                claim_key,
                schedule_id: canonical_schedule_id.clone(),
                interval: trigger.interval.clone(),
                timezone: trigger.timezone.clone(),
                due_tick_ms: *due_tick_ms,
                tick_ms,
                event_id,
                supervisor_signal: scheduling_signal.clone(),
                runtime_injection,
                runtime_injection_error,
            });
        }

        Ok(ScheduleDispatchReport {
            tick_ms,
            due_count: due_triggers.len(),
            dispatched,
            skipped_claims,
        })
    }
    fn record_schedule_claim(&mut self, claim_key: String, tick_ms: u64) -> bool {
        if !self.scheduling_claims.insert(claim_key.clone()) {
            return false;
        }
        self.scheduling_claim_ticks
            .entry(tick_ms)
            .or_default()
            .push(claim_key);
        true
    }

    fn prune_schedule_claims(&mut self, current_tick_ms: u64) {
        let cutoff_tick = current_tick_ms.saturating_sub(SCHEDULING_CLAIM_RETENTION_WINDOW_MS);
        let expired_ticks = self
            .scheduling_claim_ticks
            .keys()
            .copied()
            .take_while(|tick| *tick < cutoff_tick)
            .collect::<Vec<_>>();
        for tick in expired_ticks {
            if let Some(keys) = self.scheduling_claim_ticks.remove(&tick) {
                for key in keys {
                    self.scheduling_claims.remove(&key);
                }
            }
        }

        while self.scheduling_claims.len() > SCHEDULING_CLAIMS_MAX_RETAINED {
            let Some(oldest_tick) = self.scheduling_claim_ticks.keys().next().copied() else {
                break;
            };
            if let Some(keys) = self.scheduling_claim_ticks.remove(&oldest_tick) {
                for key in keys {
                    self.scheduling_claims.remove(&key);
                }
            } else {
                break;
            }
        }
    }

    fn prune_scheduling_last_due_ticks(&mut self, current_tick_ms: u64) {
        let cutoff_tick = current_tick_ms.saturating_sub(SCHEDULING_CLAIM_RETENTION_WINDOW_MS);
        self.scheduling_last_due_ticks
            .retain(|_, due_tick| *due_tick >= cutoff_tick);

        while self.scheduling_last_due_ticks.len() > SCHEDULING_LAST_DUE_MAX_RETAINED {
            let Some(oldest_schedule_id) = self
                .scheduling_last_due_ticks
                .iter()
                .min_by(|(left_id, left_due), (right_id, right_due)| {
                    left_due.cmp(right_due).then_with(|| left_id.cmp(right_id))
                })
                .map(|(schedule_id, _)| schedule_id.clone())
            else {
                break;
            };
            self.scheduling_last_due_ticks.remove(&oldest_schedule_id);
        }
    }
    fn scheduling_supervisor_signal(&self) -> Option<SchedulingSupervisorSignal> {
        let module_transitions = self
            .supervisor_report
            .transitions
            .iter()
            .filter(|transition| transition.module_id == "scheduling")
            .collect::<Vec<_>>();
        let latest = module_transitions.last()?;
        let restart_observed = module_transitions
            .iter()
            .any(|transition| transition.to == ModuleHealthState::Restarting);
        Some(SchedulingSupervisorSignal {
            module_id: latest.module_id.clone(),
            latest_state: latest.to.clone(),
            latest_attempt: latest.attempt,
            restart_observed,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParsedInterval {
    Marker { interval_ms: u64 },
    Cron(CronExpression),
}

impl ParsedInterval {
    fn jitter_base_interval_ms(&self) -> u64 {
        match self {
            Self::Marker { interval_ms } => *interval_ms,
            // Five-field cron expressions are minute-based.
            Self::Cron(_) => 60_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParsedTimezone {
    FixedOffsetMs(i64),
    Iana(chrono_tz::Tz),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CronExpression {
    minute: CronFieldSet,
    hour: CronFieldSet,
    day_of_month: CronFieldSet,
    month: CronFieldSet,
    day_of_week: CronFieldSet,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CronFieldSet {
    any: bool,
    min: u32,
    allowed: Vec<bool>,
}

impl CronExpression {
    fn parse(expression: &str) -> Option<Self> {
        let fields = expression.split_whitespace().collect::<Vec<_>>();
        if fields.len() != 5 {
            return None;
        }
        let parsed = Self {
            minute: parse_cron_field(fields[0], 0, 59, false)?,
            hour: parse_cron_field(fields[1], 0, 23, false)?,
            day_of_month: parse_cron_field(fields[2], 1, 31, false)?,
            month: parse_cron_field(fields[3], 1, 12, false)?,
            day_of_week: parse_cron_field(fields[4], 0, 7, true)?,
        };

        // Keep standard DOM/DOW OR semantics. Only reject expressions that can never fire
        // when day-of-week is wildcard and the selected day-of-month never exists in selected months.
        if parsed.day_of_week.any
            && !parsed.day_of_month.any
            && !parsed.has_possible_day_of_month_for_selected_months()
        {
            return None;
        }

        Some(parsed)
    }

    fn matches(&self, local: &LocalDateTimeFields) -> bool {
        if !self.minute.matches(local.minute)
            || !self.hour.matches(local.hour)
            || !self.month.matches(local.month)
        {
            return false;
        }

        let dom_match = self.day_of_month.matches(local.day_of_month);
        let dow_match = self.day_of_week.matches(local.day_of_week);

        if self.day_of_month.any && self.day_of_week.any {
            true
        } else if self.day_of_month.any {
            dow_match
        } else if self.day_of_week.any {
            dom_match
        } else {
            dom_match || dow_match
        }
    }

    fn has_possible_day_of_month_for_selected_months(&self) -> bool {
        for month in 1..=12 {
            if !self.month.matches(month) {
                continue;
            }
            let max_day = max_day_for_month_with_feb_29(month);
            for day in 1..=max_day {
                if self.day_of_month.matches(day) {
                    return true;
                }
            }
        }
        false
    }
}

impl CronFieldSet {
    fn matches(&self, value: u32) -> bool {
        if value < self.min {
            return false;
        }
        let idx = (value - self.min) as usize;
        self.allowed.get(idx).copied().unwrap_or(false)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LocalDateTimeFields {
    minute: u32,
    hour: u32,
    day_of_month: u32,
    month: u32,
    day_of_week: u32,
    second: u32,
    subsec_nanos: u32,
}

fn parse_cron_field(
    field: &str,
    min: u32,
    max: u32,
    map_sunday_seven_to_zero: bool,
) -> Option<CronFieldSet> {
    let mut allowed = vec![false; (max - min + 1) as usize];

    for raw_token in field.split(',') {
        let token = raw_token.trim();
        if token.is_empty() {
            return None;
        }
        let (base, step) = match token.split_once('/') {
            Some((base, step)) => {
                let step = step.parse::<u32>().ok()?;
                if step == 0 {
                    return None;
                }
                (base.trim(), step)
            }
            None => (token, 1),
        };

        if base == "*" {
            let mut value = min;
            while value <= max {
                let mapped = normalize_cron_value(value, map_sunday_seven_to_zero);
                let idx = (mapped - min) as usize;
                allowed[idx] = true;
                match value.checked_add(step) {
                    Some(next) => value = next,
                    None => break,
                }
            }
            continue;
        }

        if let Some((start, end)) = base.split_once('-') {
            let start = parse_cron_raw_value(start.trim(), min, max)?;
            let end = parse_cron_raw_value(end.trim(), min, max)?;
            if start > end {
                return None;
            }
            let mut value = start;
            while value <= end {
                let mapped = normalize_cron_value(value, map_sunday_seven_to_zero);
                let idx = (mapped - min) as usize;
                allowed[idx] = true;
                match value.checked_add(step) {
                    Some(next) => value = next,
                    None => break,
                }
            }
            continue;
        }

        let value = parse_cron_value(base, min, max, map_sunday_seven_to_zero)?;
        let idx = (value - min) as usize;
        allowed[idx] = true;
    }

    if allowed.iter().all(|allowed| !allowed) {
        return None;
    }

    let any = cron_field_is_semantic_wildcard(min, max, map_sunday_seven_to_zero, &allowed);
    Some(CronFieldSet { any, min, allowed })
}

fn cron_field_is_semantic_wildcard(
    min: u32,
    max: u32,
    map_sunday_seven_to_zero: bool,
    allowed: &[bool],
) -> bool {
    let mut covered = vec![false; allowed.len()];
    for raw in min..=max {
        let mapped = normalize_cron_value(raw, map_sunday_seven_to_zero);
        if mapped < min || mapped > max {
            return false;
        }
        let mapped_idx = (mapped - min) as usize;
        covered[mapped_idx] = true;
    }

    covered
        .iter()
        .enumerate()
        .filter(|(_, is_semantic_value)| **is_semantic_value)
        .all(|(idx, _)| allowed.get(idx).copied().unwrap_or(false))
}

fn parse_cron_value(raw: &str, min: u32, max: u32, map_sunday_seven_to_zero: bool) -> Option<u32> {
    let value = normalize_cron_value(
        parse_cron_raw_value(raw, min, max)?,
        map_sunday_seven_to_zero,
    );
    if value < min || value > max {
        return None;
    }
    Some(value)
}

fn parse_cron_raw_value(raw: &str, min: u32, max: u32) -> Option<u32> {
    let value = raw.parse::<u32>().ok()?;
    if value < min || value > max {
        return None;
    }
    Some(value)
}

fn normalize_cron_value(value: u32, map_sunday_seven_to_zero: bool) -> u32 {
    if map_sunday_seven_to_zero && value == 7 {
        0
    } else {
        value
    }
}

fn max_day_for_month_with_feb_29(month: u32) -> u32 {
    match month {
        2 => 29,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn parse_interval_marker_ms(interval: &str) -> Option<u64> {
    let marker = interval.trim().to_ascii_lowercase();
    let marker = marker.strip_prefix("*/")?;
    if marker.len() < 2 {
        return None;
    }
    let (count_part, unit_part) = marker.split_at(marker.len() - 1);
    let count = count_part.parse::<u64>().ok()?;
    if count == 0 {
        return None;
    }
    let unit_ms = match unit_part {
        "s" => 1_000,
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => 86_400_000,
        _ => return None,
    };
    count.checked_mul(unit_ms)
}

fn parse_schedule_interval(interval: &str) -> Option<ParsedInterval> {
    parse_interval_marker_ms(interval)
        .map(|interval_ms| ParsedInterval::Marker { interval_ms })
        .or_else(|| CronExpression::parse(interval.trim()).map(ParsedInterval::Cron))
}

fn deterministic_jitter_offset_ms(schedule_id: &str, jitter_ms: u64, interval_ms: u64) -> u64 {
    if jitter_ms == 0 || interval_ms <= 1 {
        return 0;
    }
    let mut hash = 1_469_598_103_934_665_603_u64;
    for byte in schedule_id.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    let max_jitter = jitter_ms.min(interval_ms.saturating_sub(1));
    hash % (max_jitter + 1)
}

fn parse_schedule_timezone(timezone: &str) -> Option<ParsedTimezone> {
    let timezone = timezone.trim();
    if timezone.is_empty() {
        return None;
    }
    parse_timezone_offset_ms(timezone)
        .map(ParsedTimezone::FixedOffsetMs)
        .or_else(|| {
            timezone
                .parse::<chrono_tz::Tz>()
                .ok()
                .map(ParsedTimezone::Iana)
        })
}

fn parse_timezone_offset_ms(timezone: &str) -> Option<i64> {
    let tz = timezone.trim();
    if tz.is_empty() {
        return None;
    }
    if tz.eq_ignore_ascii_case("utc") || tz == "Z" {
        return Some(0);
    }
    let offset = tz
        .strip_prefix("UTC")
        .or_else(|| tz.strip_prefix("utc"))
        .or_else(|| tz.strip_prefix("GMT"))
        .or_else(|| tz.strip_prefix("gmt"))
        .unwrap_or(tz);
    parse_hhmm_offset(offset)
}

fn parse_hhmm_offset(offset: &str) -> Option<i64> {
    if offset.is_empty() {
        return Some(0);
    }
    let sign = if offset.starts_with('+') {
        1_i64
    } else if offset.starts_with('-') {
        -1_i64
    } else {
        return None;
    };
    let body = &offset[1..];
    let (hours, minutes) = if let Some((h, m)) = body.split_once(':') {
        (h, m)
    } else if body.len() == 4 {
        body.split_at(2)
    } else {
        return None;
    };
    let hours = hours.parse::<i64>().ok()?;
    let minutes = minutes.parse::<i64>().ok()?;
    if hours > 23 || minutes > 59 {
        return None;
    }
    let total_minutes = hours.saturating_mul(60).saturating_add(minutes);
    Some(sign.saturating_mul(total_minutes).saturating_mul(60_000))
}

fn utc_datetime_from_tick_ms(tick_ms: u64) -> Option<chrono::DateTime<Utc>> {
    let tick_ms = i64::try_from(tick_ms).ok()?;
    chrono::DateTime::<Utc>::from_timestamp_millis(tick_ms)
}

fn local_fields_at_tick(timezone: &ParsedTimezone, tick_ms: u64) -> Option<LocalDateTimeFields> {
    let utc = utc_datetime_from_tick_ms(tick_ms)?;
    let (minute, hour, day_of_month, month, day_of_week, second, subsec_nanos) = match timezone {
        ParsedTimezone::FixedOffsetMs(offset_ms) => {
            let offset_seconds = i32::try_from(offset_ms / 1_000).ok()?;
            let offset = chrono::FixedOffset::east_opt(offset_seconds)?;
            let local = utc.with_timezone(&offset);
            (
                local.minute(),
                local.hour(),
                local.day(),
                local.month(),
                local.weekday().num_days_from_sunday(),
                local.second(),
                local.nanosecond(),
            )
        }
        ParsedTimezone::Iana(timezone) => {
            let local = utc.with_timezone(timezone);
            (
                local.minute(),
                local.hour(),
                local.day(),
                local.month(),
                local.weekday().num_days_from_sunday(),
                local.second(),
                local.nanosecond(),
            )
        }
    };
    Some(LocalDateTimeFields {
        minute,
        hour,
        day_of_month,
        month,
        day_of_week,
        second,
        subsec_nanos,
    })
}

fn timezone_offset_ms_at_tick(timezone: &ParsedTimezone, tick_ms: u64) -> Option<i64> {
    match timezone {
        ParsedTimezone::FixedOffsetMs(offset) => Some(*offset),
        ParsedTimezone::Iana(tz) => {
            let utc = utc_datetime_from_tick_ms(tick_ms)?;
            let local = utc.with_timezone(tz);
            Some(i64::from(local.offset().fix().local_minus_utc()).saturating_mul(1_000))
        }
    }
}

fn latest_due_marker_tick_at_or_before(
    interval_ms: u64,
    timezone: &ParsedTimezone,
    tick_ms: u64,
) -> Option<u64> {
    match timezone {
        ParsedTimezone::FixedOffsetMs(timezone_offset_ms) => {
            latest_due_marker_tick_at_or_before_with_offset(
                interval_ms,
                *timezone_offset_ms,
                tick_ms,
            )
        }
        ParsedTimezone::Iana(_) => {
            let mut timezone_offset_ms = timezone_offset_ms_at_tick(timezone, tick_ms)?;
            for _ in 0..4 {
                let due_tick = latest_due_marker_tick_at_or_before_with_offset(
                    interval_ms,
                    timezone_offset_ms,
                    tick_ms,
                )?;
                let due_offset_ms = timezone_offset_ms_at_tick(timezone, due_tick)?;
                if due_offset_ms == timezone_offset_ms {
                    return Some(due_tick);
                }
                timezone_offset_ms = due_offset_ms;
            }
            latest_due_marker_tick_at_or_before_with_offset(
                interval_ms,
                timezone_offset_ms,
                tick_ms,
            )
        }
    }
}

fn latest_due_marker_tick_at_or_before_with_offset(
    interval_ms: u64,
    timezone_offset_ms: i64,
    tick_ms: u64,
) -> Option<u64> {
    let local_tick = i128::from(tick_ms) + i128::from(timezone_offset_ms);
    if local_tick < 0 {
        return None;
    }
    let local_tick = local_tick as u64;
    let latest_due_local_tick = local_tick - (local_tick % interval_ms);
    let due_tick = i128::from(latest_due_local_tick) - i128::from(timezone_offset_ms);
    if due_tick < 0 {
        return None;
    }
    Some(due_tick as u64)
}

fn canonical_schedule_id(schedule_id: &str) -> String {
    schedule_id.trim().to_string()
}

fn validate_schedule_tick_ms_supported(tick_ms: u64) -> Result<(), ScheduleValidationError> {
    if tick_ms > i64::MAX as u64 {
        return Err(ScheduleValidationError::InvalidTickMs(tick_ms));
    }
    Ok(())
}

fn latest_due_cron_tick_at_or_before(
    cron: &CronExpression,
    timezone: &ParsedTimezone,
    tick_ms: u64,
) -> Option<u64> {
    let mut candidate = tick_ms - (tick_ms % 60_000);
    for _ in 0..=CRON_LOOKBACK_MINUTES {
        let fields = local_fields_at_tick(timezone, candidate)?;
        if fields.second == 0 && fields.subsec_nanos == 0 && cron.matches(&fields) {
            return Some(candidate);
        }
        candidate = candidate.checked_sub(60_000)?;
    }
    None
}

fn latest_due_tick_at_or_before(
    schedule_id: &str,
    interval: &ParsedInterval,
    timezone: &ParsedTimezone,
    jitter_ms: u64,
    tick_ms: u64,
) -> Option<u64> {
    let jitter_offset_ms =
        deterministic_jitter_offset_ms(schedule_id, jitter_ms, interval.jitter_base_interval_ms());
    let tick_without_jitter = tick_ms.checked_sub(jitter_offset_ms)?;
    let due_without_jitter = match interval {
        ParsedInterval::Marker { interval_ms } => {
            latest_due_marker_tick_at_or_before(*interval_ms, timezone, tick_without_jitter)?
        }
        ParsedInterval::Cron(cron) => {
            latest_due_cron_tick_at_or_before(cron, timezone, tick_without_jitter)?
        }
    };
    due_without_jitter.checked_add(jitter_offset_ms)
}
