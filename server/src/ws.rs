use crate::{
    auth::WsTokenClaims,
    errors::{ErrorCode, ServerError},
    http::HttpState,
    protocol::{
        self, DifficultyConfig, ErrorPayload, InboundMessage, OutboundMessage, PongPayload,
        SessionConfig, SnapshotEvent, SnapshotPayload, SnapshotStats, StartPayload, WelcomePayload,
        WorldConfig,
    },
};
use axum::{
    extract::{ws::WebSocketUpgrade, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use futures_util::StreamExt;
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
                Json(ErrorPayload {
                    code: ErrorCode::Unauthorized,
                    message: "missing token".to_string(),
                }),
            )
                .into_response();
        }
    };
    let claims = match state.token_issuer.verify_ws_token(&token) {
        Ok(claims) => claims,
        Err(err) => return err.into_response(),
    };
    if state
        .matchmaker
        .player_name(&claims.room_id, &claims.player_id)
        .is_none()
    {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErrorPayload {
                code: ErrorCode::Unauthorized,
                message: "unknown player".to_string(),
            }),
        )
            .into_response();
    }
    let state_arc = Arc::clone(&state);
    ws.on_upgrade(move |socket| async move {
        if let Err(err) = drive_socket(socket, state_arc, claims).await {
            tracing::warn!(error = ?err, "websocket session ended with error");
        }
    })
}

async fn drive_socket(
    mut socket: axum::extract::ws::WebSocket,
    state: Arc<HttpState>,
    claims: WsTokenClaims,
) -> Result<(), ServerError> {
    let mut seq_counter: u64 = 1;
    let mut last_ack_tick = 0;
    let mut last_input_seq = 0;
    let mut joined = false;

    let seed = state
        .matchmaker
        .room_seed(&claims.room_id)
        .ok_or_else(|| ServerError::new(ErrorCode::RoomNotFound, "room not found"))?;

    while let Some(result) = socket.next().await {
        let message = match result {
            Ok(message) => message,
            Err(err) => {
                return Err(ServerError::with_source(
                    ErrorCode::Internal,
                    "websocket receive error",
                    err.into(),
                ))
            }
        };

        match message {
            axum::extract::ws::Message::Text(text) => {
                let inbound: InboundMessage = serde_json::from_str(&text).map_err(|err| {
                    ServerError::with_source(ErrorCode::InvalidState, "invalid json", err.into())
                })?;

                if inbound.meta().pv != protocol::SERVER_PV {
                    send_message(
                        &mut socket,
                        OutboundMessage::new_error(
                            seq_counter,
                            ErrorPayload {
                                code: ErrorCode::BadVersion,
                                message: format!(
                                    "Server pv={}, client pv={}",
                                    protocol::SERVER_PV,
                                    inbound.meta().pv
                                ),
                            },
                        ),
                    )
                    .await?;
                    seq_counter += 1;
                    break;
                }

                match inbound {
                    InboundMessage::Join { payload, .. } => {
                        if joined {
                            send_message(
                                &mut socket,
                                OutboundMessage::new_error(
                                    seq_counter,
                                    ErrorPayload {
                                        code: ErrorCode::InvalidState,
                                        message: "join already processed".to_string(),
                                    },
                                ),
                            )
                            .await?;
                            seq_counter += 1;
                            continue;
                        }
                        joined = true;
                        let resume_token = state
                            .token_issuer
                            .mint_resume_token(&claims.room_id, &claims.player_id);
                        state
                            .matchmaker
                            .set_resume_token(
                                &claims.room_id,
                                &claims.player_id,
                                resume_token.0.clone(),
                            )
                            .await;

                        let mut feature_flags = HashMap::new();
                        feature_flags.insert("enemies".to_string(), false);
                        feature_flags.insert("movingPlatforms".to_string(), true);

                        let welcome = WelcomePayload {
                            player_id: claims.player_id.clone(),
                            resume_token: resume_token.0,
                            room_id: claims.room_id.clone(),
                            seed,
                            cfg: session_config(&state),
                            feature_flags,
                        };
                        send_message(
                            &mut socket,
                            OutboundMessage::new_welcome(seq_counter, welcome),
                        )
                        .await?;
                        seq_counter += 1;

                        let start_payload = StartPayload {
                            start_tick: 0,
                            server_tick: 0,
                            server_time_ms: current_ts_millis(),
                            tps: state.config.tick_rate_hz,
                        };
                        send_message(
                            &mut socket,
                            OutboundMessage::new_start(seq_counter, start_payload),
                        )
                        .await?;
                        seq_counter += 1;

                        let snapshot = SnapshotPayload {
                            tick: 0,
                            ack_tick: last_ack_tick,
                            last_input_seq,
                            full: true,
                            players: vec![],
                            events: vec![],
                            stats: SnapshotStats::default(),
                        };
                        send_message(
                            &mut socket,
                            OutboundMessage::new_snapshot(seq_counter, snapshot),
                        )
                        .await?;
                        seq_counter += 1;

                        tracing::info!(
                            player = %payload.name,
                            room = %claims.room_id,
                            "player joined"
                        );
                    }
                    InboundMessage::Input { ref payload, .. } => {
                        if !joined {
                            send_message(
                                &mut socket,
                                OutboundMessage::new_error(
                                    seq_counter,
                                    ErrorPayload {
                                        code: ErrorCode::InvalidState,
                                        message: "must join before sending input".to_string(),
                                    },
                                ),
                            )
                            .await?;
                            seq_counter += 1;
                            continue;
                        }
                        last_ack_tick = payload.tick;
                        last_input_seq = inbound.meta().seq;
                        let ack_snapshot = SnapshotPayload {
                            tick: last_ack_tick,
                            ack_tick: last_ack_tick,
                            last_input_seq,
                            full: false,
                            players: Vec::new(),
                            events: Vec::new(),
                            stats: SnapshotStats::default(),
                        };
                        send_message(
                            &mut socket,
                            OutboundMessage::new_snapshot(seq_counter, ack_snapshot),
                        )
                        .await?;
                        seq_counter += 1;
                    }
                    InboundMessage::InputBatch { ref payload, .. } => {
                        if !joined {
                            send_message(
                                &mut socket,
                                OutboundMessage::new_error(
                                    seq_counter,
                                    ErrorPayload {
                                        code: ErrorCode::InvalidState,
                                        message: "must join before sending input".to_string(),
                                    },
                                ),
                            )
                            .await?;
                            seq_counter += 1;
                            continue;
                        }
                        last_ack_tick = payload
                            .frames
                            .last()
                            .map(|frame| payload.start_tick + frame.d as u64)
                            .unwrap_or(payload.start_tick);
                        last_input_seq = inbound.meta().seq;
                        let ack_snapshot = SnapshotPayload {
                            tick: last_ack_tick,
                            ack_tick: last_ack_tick,
                            last_input_seq,
                            full: false,
                            players: Vec::new(),
                            events: Vec::new(),
                            stats: SnapshotStats::default(),
                        };
                        send_message(
                            &mut socket,
                            OutboundMessage::new_snapshot(seq_counter, ack_snapshot),
                        )
                        .await?;
                        seq_counter += 1;
                    }
                    InboundMessage::Ping { payload, .. } => {
                        let pong = PongPayload {
                            t0: payload.t0,
                            t1: current_ts_millis(),
                            t2: Some(current_ts_millis()),
                        };
                        send_message(&mut socket, OutboundMessage::new_pong(seq_counter, pong))
                            .await?;
                        seq_counter += 1;
                    }
                    InboundMessage::Reconnect { payload, .. } => {
                        if !state
                            .matchmaker
                            .validate_resume_token(
                                &claims.room_id,
                                &payload.player_id,
                                &payload.resume_token,
                            )
                            .await
                        {
                            send_message(
                                &mut socket,
                                OutboundMessage::new_error(
                                    seq_counter,
                                    ErrorPayload {
                                        code: ErrorCode::Unauthorized,
                                        message: "invalid resume token".to_string(),
                                    },
                                ),
                            )
                            .await?;
                            seq_counter += 1;
                            continue;
                        }
                        let snapshot = SnapshotPayload {
                            tick: payload.last_ack_tick,
                            ack_tick: payload.last_ack_tick,
                            last_input_seq,
                            full: true,
                            players: vec![],
                            events: vec![SnapshotEvent {
                                kind: "resume".to_string(),
                                x: 0.0,
                                y: 0.0,
                                tick: payload.last_ack_tick,
                            }],
                            stats: SnapshotStats::default(),
                        };
                        send_message(
                            &mut socket,
                            OutboundMessage::new_snapshot(seq_counter, snapshot),
                        )
                        .await?;
                        seq_counter += 1;
                    }
                }
            }
            axum::extract::ws::Message::Binary(_) => {
                send_message(
                    &mut socket,
                    OutboundMessage::new_error(
                        seq_counter,
                        ErrorPayload {
                            code: ErrorCode::InvalidState,
                            message: "binary frames are not supported".to_string(),
                        },
                    ),
                )
                .await?;
                seq_counter += 1;
            }
            axum::extract::ws::Message::Ping(payload) => {
                socket
                    .send(axum::extract::ws::Message::Pong(payload))
                    .await
                    .map_err(|err| {
                        ServerError::with_source(
                            ErrorCode::Internal,
                            "failed to send pong",
                            err.into(),
                        )
                    })?;
            }
            axum::extract::ws::Message::Pong(_) => {}
            axum::extract::ws::Message::Close(_) => {
                break;
            }
        }
    }

    if joined {
        let finish = OutboundMessage::Finish {
            meta: protocol::MessageMeta::new(seq_counter),
            payload: protocol::FinishPayload {
                reason: "room_closed".to_string(),
            },
        };
        let _ = send_message(&mut socket, finish).await;
    }

    Ok(())
}

async fn send_message(
    socket: &mut axum::extract::ws::WebSocket,
    message: OutboundMessage,
) -> Result<(), ServerError> {
    let text = serde_json::to_string(&message).map_err(|err| {
        ServerError::with_source(ErrorCode::Internal, "serialization error", err.into())
    })?;
    socket
        .send(axum::extract::ws::Message::Text(text))
        .await
        .map_err(|err| ServerError::with_source(ErrorCode::Internal, "failed to send", err.into()))
}

fn session_config(state: &HttpState) -> SessionConfig {
    SessionConfig {
        tps: state.config.tick_rate_hz,
        snapshot_rate_hz: state.config.snapshot_rate_hz,
        max_rollback_ticks: state.config.max_rollback_ticks,
        input_lead_ticks: state.config.input_lead_ticks,
        world: Some(WorldConfig {
            world_width: 1080.0,
            platform_width: 120.0,
            platform_height: 18.0,
            gap_min: 120.0,
            gap_max: 240.0,
            gravity: -2200.0,
            jump_vy: 1200.0,
            spring_vy: 1800.0,
            max_vx: 900.0,
            tilt_accel: 1200.0,
        }),
        difficulty: Some(DifficultyConfig {
            gap_min_start: 120.0,
            gap_min_end: 180.0,
            gap_max_start: 240.0,
            gap_max_end: 320.0,
            spring_chance_start: 0.1,
            spring_chance_end: 0.03,
        }),
    }
}

fn current_ts_millis() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
