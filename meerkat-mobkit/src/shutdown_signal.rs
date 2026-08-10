//! Operator termination signal for long-lived MobKit binaries.
//!
//! # Why this module exists
//!
//! [`tokio::signal::ctrl_c`] listens for **SIGINT and nothing else**. Both
//! gateway binaries waited on it alone, which is correct for an interactive
//! ctrl-c and wrong for every other way a long-lived process is stopped:
//! Kubernetes, systemd and `docker stop` all send **SIGTERM** first and
//! escalate to SIGKILL only after a grace period. SIGTERM's default
//! disposition terminates the process immediately, so a `ctrl_c`-only wait
//! means the graceful shutdown path never runs on an ordinary deploy.
//!
//! That was survivable until meerkat 0.8.22 introduced the schedule executor
//! lease, which made an ungraceful exit *cost* something durable. The lease is
//! released only on the graceful path (`ScheduleHostHandle::shutdown`). Skip
//! it and the row keeps a future `expires_at_utc`, so the replacement process
//! gets `AcquireScheduleExecutorLeaseOutcome::Busy`, its tick returns without
//! calling `claim_due_occurrences`, and **schedules do not fire for up to
//! `lease_duration` (60s by default) after every restart**. The claim watchdog
//! cannot see it either: its overdue threshold is 2 minutes, longer than the
//! window it would need to observe.
//!
//! So this is not a tidiness fix. On a container platform the pre-0.8.22
//! behaviour was "an ungraceful stop loses nothing"; after 0.8.22 it is "every
//! deploy silently stops firing schedules for a minute".
//!
//! # Contract
//!
//! Resolves on the FIRST of SIGINT or SIGTERM. It does not tell the caller
//! which arrived, because no caller has a reason to behave differently - both
//! mean "an operator or supervisor is stopping this process, run the shutdown
//! sequence". Callers keep owning what shutdown means; this only decides when.
//!
//! If the SIGTERM handler cannot be installed the function degrades to SIGINT
//! only rather than failing the process. A binary that refuses to start
//! because it could not register a signal handler is strictly worse than one
//! that starts and handles fewer signals.

/// Resolve when the process is asked to terminate, by SIGINT or SIGTERM.
///
/// See the module docs for why waiting on [`tokio::signal::ctrl_c`] alone is
/// insufficient for any process that will be deployed in a container.
#[cfg(unix)]
pub async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};

    let mut terminate = match signal(SignalKind::terminate()) {
        Ok(stream) => stream,
        Err(error) => {
            // Degrade, do not abort: SIGINT coverage is still better than
            // exiting here, and an operator ctrl-c must keep working.
            //
            // `eprintln!` rather than `tracing::warn!` is deliberate - please
            // do not "fix" it. This fires at most once per process, and it
            // reports that a shutdown path is degraded, which is exactly the
            // class of message a log filter must not be able to swallow. A
            // downstream fleet lost 13 days to a leftover `RUST_LOG=warn`
            // discarding the only lines that named a silent failure; a WARN
            // here would survive that filter but not `RUST_LOG=error`, and
            // tracing may not even be initialised at this point in startup.
            eprintln!(
                "warning: could not install a SIGTERM handler ({error}); shutting down cleanly on \
                 SIGINT only. A container stop signal will terminate this process without \
                 releasing the schedule executor lease."
            );
            let _ = tokio::signal::ctrl_c().await;
            return;
        }
    };

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
}

/// Non-Unix fallback: SIGTERM has no equivalent, so this is SIGINT only.
#[cfg(not(unix))]
pub async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
