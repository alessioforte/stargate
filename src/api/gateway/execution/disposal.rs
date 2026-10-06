use crate::api::gateway::lifecycle::Execution;
use crate::{err::ErrorCode, etc::observability::telemetry};
use axum::{body::Body, response::Response};
use http_body_util::BodyExt;
use std::time::Duration;
use tokio::time::Instant;

#[cfg(test)]
pub(in crate::api::gateway) const DISCARDED_BODY_TIMEOUT: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::api::gateway) enum DisposalOutcome {
    Drained,
    ByteLimit,
    Timeout,
    BodyError,
    Cancelled,
}

impl DisposalOutcome {
    fn label(self) -> &'static str {
        match self {
            Self::Drained => "drained",
            Self::ByteLimit => "byte_limit",
            Self::Timeout => "timeout",
            Self::BodyError => "body_error",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct DisposalResult {
    outcome: DisposalOutcome,
    bytes: usize,
}

pub(in crate::api::gateway) async fn discard_response(
    response: Response,
    kind: &'static str,
    settings: &gate::cfg::CompiledRuntimeSettings,
    execution: &crate::api::gateway::lifecycle::Execution,
) -> DisposalOutcome {
    let status = response.status().as_u16();
    let started = Instant::now();
    let result = discard_body(
        response.into_body(),
        settings.discarded_body_bytes,
        settings.discarded_body_timeout,
        execution.clone(),
    )
    .await;
    telemetry::record_gateway_response_disposal(
        kind,
        result.outcome.label(),
        result.bytes,
        started.elapsed(),
    );
    if result.outcome != DisposalOutcome::Drained {
        tracing::debug!(
            kind,
            status,
            outcome = result.outcome.label(),
            bytes = result.bytes,
            "Discarded response body dropped before completion"
        );
    }
    result.outcome
}

async fn discard_body(
    mut body: Body,
    max_bytes: usize,
    timeout: Duration,
    execution: Execution,
) -> DisposalResult {
    let deadline = Instant::now() + timeout;
    let mut bytes = 0_usize;
    let outcome = loop {
        // Always-ready empty frames must yield too, and progress cannot reset
        // the absolute disposal deadline.
        tokio::task::consume_budget().await;
        if Instant::now() >= deadline {
            execution.timeout("disposal");
            break DisposalOutcome::Timeout;
        }
        match execution
            .run(
                "disposal",
                deadline.saturating_duration_since(Instant::now()),
                body.frame(),
            )
            .await
        {
            Ok(Some(Ok(frame))) => {
                if let Some(data) = frame.data_ref() {
                    bytes = bytes.saturating_add(data.len());
                    // One already-received frame may cross the byte budget.
                    // Never retain it or poll another frame after reaching it.
                    if bytes >= max_bytes {
                        break DisposalOutcome::ByteLimit;
                    }
                }
            }
            Ok(None) => break DisposalOutcome::Drained,
            Ok(Some(Err(_))) => {
                break if execution.failure().is_some() {
                    DisposalOutcome::Timeout
                } else {
                    DisposalOutcome::BodyError
                };
            }
            Err(error) if error.code == ErrorCode::GatewayCancelled => {
                break DisposalOutcome::Cancelled;
            }
            Err(_) => break DisposalOutcome::Timeout,
        }
    };
    // Own the body so every exit, including cancellation, releases it.
    DisposalResult { outcome, bytes }
}

#[cfg(test)]
mod tests;
