//! A device's inbox: paged with `GET /inbox/:after`, live with `GET /stream` and `Last-Event-ID`.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use axum::{Json, Router};
use buccanet_protocol::api::InboxPage;
use futures::Stream;

use crate::auth::AuthDevice;
use crate::error::AppResult;
use crate::state::AppState;

const HEARTBEAT: Duration = Duration::from_secs(25);

pub fn routes() -> Router<AppState> {
    Router::new().route("/inbox/{after}", get(page)).route("/stream", get(stream))
}

async fn page(State(state): State<AppState>, AuthDevice(device): AuthDevice, Path(after): Path<u64>) -> AppResult<Json<InboxPage>> {
    let events = state.inbox.after(&device.id, after as i64).await?;
    Ok(Json(InboxPage { events }))
}

fn last_event_id(headers: &HeaderMap) -> i64 {
    headers.get("last-event-id").and_then(|value| value.to_str().ok()).and_then(|value| value.parse().ok()).unwrap_or(0)
}

async fn stream(
    State(state): State<AppState>,
    AuthDevice(device): AuthDevice,
    headers: HeaderMap,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut cursor = last_event_id(&headers);
    let notify = state.hub.handle(&device.id);
    let events = async_stream::stream! {
        loop {
            let woken = notify.notified();
            tokio::pin!(woken);
            woken.as_mut().enable();
            let Ok(batch) = state.inbox.after(&device.id, cursor).await else { break };
            for event in batch {
                cursor = event.cursor as i64;
                let data = serde_json::to_string(&event).expect("inbox events always serialize");
                yield Ok(Event::default().id(cursor.to_string()).data(data));
            }
            let _ = tokio::time::timeout(HEARTBEAT, woken).await;
        }
    };
    Sse::new(events).keep_alive(KeepAlive::new().interval(HEARTBEAT))
}
