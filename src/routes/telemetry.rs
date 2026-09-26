//! Client activity trace: what the browser sees and the API does not. Not analytics: a
//! tap that hits a cache or a disabled control never reaches the server, so "I pressed
//! it and nothing happened" is otherwise undiagnosable. Events join the API requests'
//! log stream, so a session reads as one timeline (`client-event kind=tap label="Log
//! set"`, then the `POST /api/sets` it caused). No storage: these are logs, not data.
//! The same endpoint runs in `life`.

use axum::Json;
use axum::http::StatusCode;
use serde::Deserialize;

use crate::session::AuthUser;

/// One thing that happened in the client.
///
/// `kind` is `nav` for a route change, where `label` is absent, or `tap` for a
/// control, where `label` is its visible text, verbatim.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct TelemetryEvent {
    pub kind: String,
    pub path: String,
    #[serde(default)]
    pub label: Option<String>,
    /// The client's clock, in epoch milliseconds.
    ///
    /// Kept because a batch arrives all at once, so the server's receive time
    /// cannot order the events inside it and the client's can.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub at: i64,
}

/// Most events accepted from one POST.
///
/// The real client batches a handful at a time; this stops a buggy or hostile
/// one turning a single request into a log flood.
const MAX_EVENTS: usize = 100;

/// Longest label kept, in characters.
///
/// Labels are verbatim UI text, so a pathological one would otherwise bloat a
/// log line. Counted in `chars` rather than bytes so a multi-byte glyph is never
/// split down the middle.
const MAX_LABEL: usize = 160;

/// Format characters that are invisible or reorder what is displayed: zero-width
/// characters (a label that reads as empty) and bidi overrides (U+202A–202E,
/// U+2066–2069), which make a log line display something other than what it says, as in
/// Trojan Source. `char::is_control` covers only Cc and std has no category table, so
/// this is an explicit deny-list rather than all of Cf.
fn is_deceptive_format(c: char) -> bool {
    matches!(c,
        '\u{00ad}'
        | '\u{200b}'..='\u{200f}'
        | '\u{202a}'..='\u{202e}'
        | '\u{2060}'..='\u{2064}'
        | '\u{2066}'..='\u{2069}'
        | '\u{feff}'
    )
}

/// Flatten a client-supplied label to one harmless log field. **This is the endpoint's
/// security boundary:** a newline in a label would forge whole log lines, and the log
/// would stop being evidence. Control characters become spaces (U+2028/U+2029, which
/// `is_control` misses, go through `split_whitespace`), whitespace collapses, and the
/// result is capped in chars so no glyph is split. Public so its tests can attack it
/// directly.
pub fn one_line(label: &str, max: usize) -> String {
    let unbroken: String = label
        .chars()
        .map(|c| {
            if c.is_control() || is_deceptive_format(c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    unbroken
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max)
        .collect()
}

/// `POST /api/telemetry`: fold the client's events into the log. Always 204 and
/// best-effort; auth-gated, so every line is attributed.
pub async fn record(
    AuthUser(user): AuthUser,
    Json(events): Json<Vec<TelemetryEvent>>,
) -> StatusCode {
    for e in events.into_iter().take(MAX_EVENTS) {
        let label = one_line(&e.label.unwrap_or_default(), MAX_LABEL);
        tracing::info!(
            user = %user.user_id,
            kind = %e.kind,
            path = %e.path,
            label = %label,
            at = e.at,
            "client-event"
        );
    }
    StatusCode::NO_CONTENT
}
