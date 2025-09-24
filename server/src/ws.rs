use crate::{errors::ErrorCode, http::HttpState};
use axum::{
    extract::{ws::WebSocketUpgrade, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::collections::HashMap;
use std::sync::Arc;

pub async fn upgrade_handler(
    State(state): State<Arc<HttpState>>,
    Query(params): Query<HashMap<String, String>>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let token = match params.get("token") {
        Some(token) => token.clone(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(crate::protocol::ErrorPayload {
                    code: ErrorCode::Unauthorized,
                    message: "missing token".to_string(),
                }),
            )
                .into_response();
        }
    };
    let claims = state
        .token_issuer
        .verify_ws_token(&token)
        .map_err(|err| err.into_response());
    if let Err(resp) = claims {
        return resp;
    }
    ws.on_upgrade(|_socket| async move {
        tracing::info!("ws_upgrade_stub");
    })
}
