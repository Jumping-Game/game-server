use crate::{
    broadcaster::ConnectionQueue,
    errors::{AppError, ErrorCode, WireError},
    http::HttpState,
    proto::{
        ClientInput, ClientInputBatch, ClientJoin, ClientPing, ClientReadySet, ClientReconnect,
        ClientStartRequest, Envelope, PongPayload, ServerFrame, PROTOCOL_VERSION,
    },
    util,
};
use futures::{SinkExt, StreamExt};
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio_tungstenite::{
    accept_hdr_async_with_config,
    tungstenite::{
        handshake::server::{Request, Response},
        http::{header, StatusCode as WsStatusCode},
        protocol::WebSocketConfig,
        Message,
    },
};
use tracing::{error, info, warn};
use url::form_urlencoded;

#[derive(Clone)]
pub struct WsServer {
    pub state: Arc<HttpState>,
}

impl WsServer {
    pub fn new(state: Arc<HttpState>) -> Self {
        Self { state }
    }

    pub async fn run(self) -> anyhow::Result<()> {
        let listener = TcpListener::bind(&self.state.config.ws_bind).await?;
        info!(bind = %self.state.config.ws_bind, "ws_listening");
        loop {
            let (stream, addr) = listener.accept().await?;
            let state = self.state.clone();
            tokio::spawn(async move {
                if let Err(err) = handle_socket(stream, state).await {
                    warn!(?err, ?addr, "ws_connection_error");
                }
            });
        }
    }
}

async fn handle_socket(stream: tokio::net::TcpStream, state: Arc<HttpState>) -> anyhow::Result<()> {
    let holder = Arc::new(Mutex::new(None));
    let callback_holder = holder.clone();
    let mut config = WebSocketConfig::default();
    config.max_message_size = Some(4 * 1024);
    config.max_frame_size = Some(4 * 1024);
    let ws_stream = accept_hdr_async_with_config(
        stream,
        {
            let enable_deflate = state.config.enable_permessage_deflate;
            move |req: &Request, mut resp: Response| {
                if req.uri().path() != "/v1/ws" {
                    let err = Response::builder()
                        .status(WsStatusCode::NOT_FOUND)
                        .body(Some("not found".to_string()))
                        .unwrap();
                    return Err(err);
                }
                let mut token: Option<String> = None;
                if let Some(query) = req.uri().query() {
                    for (key, value) in form_urlencoded::parse(query.as_bytes()) {
                        if key == "token" {
                            token = Some(value.into_owned());
                            break;
                        }
                    }
                }
                let Some(token) = token else {
                    let err = Response::builder()
                        .status(WsStatusCode::BAD_REQUEST)
                        .body(Some("missing token".to_string()))
                        .unwrap();
                    return Err(err);
                };
                *callback_holder.lock().unwrap() = Some(token);
                if !enable_deflate {
                    resp.headers_mut().remove(header::SEC_WEBSOCKET_EXTENSIONS);
                }
                Ok(resp)
            }
        },
        Some(config),
    )
    .await?;

    let token = holder
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| anyhow::anyhow!("missing token"))?;
    let claims = match state.auth.verify_ws_token(&token) {
        Ok(c) => c,
        Err(err) => {
            return Err(anyhow::anyhow!(err.to_string()));
        }
    };

    let (ws_sender, mut ws_receiver) = ws_stream.split();
    let queue = ConnectionQueue::new(Some(8));
    let mut joined = false;
    let mut sim_handle = None;
    let mut resume_token: Option<String> = None;

    let send_queue = queue.clone();
    let writer = tokio::spawn(async move {
        let mut sender = ws_sender;
        while let Some(frame) = send_queue.next().await {
            match serde_json::to_string(&frame) {
                Ok(payload) => {
                    if sender.send(Message::Text(payload)).await.is_err() {
                        break;
                    }
                }
                Err(err) => {
                    error!(?err, "serialize_error");
                    break;
                }
            }
        }
    });

    while let Some(msg) = ws_receiver.next().await {
        let msg = msg?;
        if msg.is_close() {
            break;
        }
        let Message::Text(text) = msg else {
            continue;
        };
        tracing::info!(room = %claims.room_id, player = %claims.player_id, payload = %text, "ws_inbound");
        let raw_env: Envelope<serde_json::Value> = match serde_json::from_str(&text) {
            Ok(env) => env,
            Err(err) => {
                send_error(
                    &state,
                    &claims.room_id,
                    &queue,
                    ErrorCode::InvalidState,
                    err.to_string(),
                )
                .await;
                continue;
            }
        };
        if raw_env.pv != PROTOCOL_VERSION {
            send_error(
                &state,
                &claims.room_id,
                &queue,
                ErrorCode::BadVersion,
                format!("expected pv={}, got {}", PROTOCOL_VERSION, raw_env.pv),
            )
            .await;
            continue;
        }
        tracing::info!(frame_type = %raw_env.kind, room = %claims.room_id, player = %claims.player_id, "ws_frame");
        match raw_env.kind.as_str() {
            "join" => {
                if joined {
                    continue;
                }
                let _payload: ClientJoin = match serde_json::from_value(raw_env.payload.clone()) {
                    Ok(p) => p,
                    Err(err) => {
                        send_error(
                            &state,
                            &claims.room_id,
                            &queue,
                            ErrorCode::InvalidState,
                            err.to_string(),
                        )
                        .await;
                        continue;
                    }
                };
                tracing::info!(room = %claims.room_id, player = %claims.player_id, "ws_join" );
                let attach = state
                    .lobby
                    .attach_connection(&claims.room_id, &claims.player_id, queue.clone())
                    .await?;
                resume_token = Some(attach.resume_token.clone());
                state
                    .resume
                    .put(attach.resume_token.clone(), claims.player_id.clone())
                    .await;
                queue.push(attach.welcome).await;
                queue.push(attach.lobby).await;
                tracing::info!(room = %claims.room_id, player = %claims.player_id, "ws_join_complete");
                sim_handle = Some(attach.sim);
                joined = true;
            }
            "ping" => {
                let payload: ClientPing = match serde_json::from_value(raw_env.payload.clone()) {
                    Ok(p) => p,
                    Err(err) => {
                        send_error(
                            &state,
                            &claims.room_id,
                            &queue,
                            ErrorCode::InvalidState,
                            err.to_string(),
                        )
                        .await;
                        continue;
                    }
                };
                if !joined {
                    continue;
                }
                let seq = next_seq(&state, &claims.room_id).await;
                let payload = PongPayload {
                    t0: payload.t0,
                    t1: util::now_ms(),
                };
                let frame = ServerFrame::Pong {
                    meta: Envelope::new("pong", seq, payload),
                };
                queue.push(frame).await;
            }
            "input" => {
                let payload: ClientInput = match serde_json::from_value(raw_env.payload.clone()) {
                    Ok(p) => p,
                    Err(err) => {
                        send_error(
                            &state,
                            &claims.room_id,
                            &queue,
                            ErrorCode::InvalidState,
                            err.to_string(),
                        )
                        .await;
                        continue;
                    }
                };
                if let Some(sim) = sim_handle.clone() {
                    sim.submit_input(
                        claims.player_id.clone(),
                        payload.tick,
                        payload.axis_x,
                        payload.jump,
                        raw_env.seq,
                    )
                    .await;
                }
            }
            "input_batch" => {
                let payload: ClientInputBatch =
                    match serde_json::from_value(raw_env.payload.clone()) {
                        Ok(p) => p,
                        Err(err) => {
                            send_error(
                                &state,
                                &claims.room_id,
                                &queue,
                                ErrorCode::InvalidState,
                                err.to_string(),
                            )
                            .await;
                            continue;
                        }
                    };
                if let Some(sim) = sim_handle.clone() {
                    handle_batch_input(&sim, &claims.player_id, &payload, raw_env.seq).await;
                }
            }
            "reconnect" => {
                let payload: ClientReconnect = match serde_json::from_value(raw_env.payload.clone())
                {
                    Ok(p) => p,
                    Err(err) => {
                        send_error(
                            &state,
                            &claims.room_id,
                            &queue,
                            ErrorCode::InvalidState,
                            err.to_string(),
                        )
                        .await;
                        continue;
                    }
                };
                if joined {
                    continue;
                }
                if state.resume.take(&payload.resume_token).await.as_deref()
                    != Some(&claims.player_id)
                {
                    send_error(
                        &state,
                        &claims.room_id,
                        &queue,
                        ErrorCode::Unauthorized,
                        "invalid resume token".to_string(),
                    )
                    .await;
                    break;
                }
                let attach = state
                    .lobby
                    .attach_connection(&claims.room_id, &claims.player_id, queue.clone())
                    .await?;
                resume_token = Some(attach.resume_token.clone());
                state
                    .resume
                    .put(attach.resume_token.clone(), claims.player_id.clone())
                    .await;
                queue.push(attach.welcome).await;
                queue.push(attach.lobby).await;
                if let Some(sim) = sim_handle.clone().or(Some(attach.sim.clone())) {
                    sim.force_full_snapshot().await;
                    sim_handle = Some(sim);
                }
                if sim_handle.is_none() {
                    sim_handle = Some(attach.sim);
                }
                joined = true;
            }
            "ready_set" => {
                let payload: ClientReadySet = match serde_json::from_value(raw_env.payload.clone())
                {
                    Ok(p) => p,
                    Err(err) => {
                        send_error(
                            &state,
                            &claims.room_id,
                            &queue,
                            ErrorCode::InvalidState,
                            err.to_string(),
                        )
                        .await;
                        continue;
                    }
                };
                let ready = payload.ready;
                if let Err(err) = state
                    .lobby
                    .set_ready(&claims.room_id, &claims.player_id, ready)
                    .await
                {
                    send_app_error(&state, &claims.room_id, &queue, err).await;
                }
            }
            "start_request" => {
                let payload: ClientStartRequest =
                    match serde_json::from_value(raw_env.payload.clone()) {
                        Ok(p) => p,
                        Err(err) => {
                            send_error(
                                &state,
                                &claims.room_id,
                                &queue,
                                ErrorCode::InvalidState,
                                err.to_string(),
                            )
                            .await;
                            continue;
                        }
                    };
                let countdown = util::clamp_countdown(payload.countdown_sec.unwrap_or(3));
                if let Err(err) = state
                    .lobby
                    .start_room(
                        &claims.room_id,
                        &claims.player_id,
                        countdown,
                        state.config.require_ready,
                    )
                    .await
                {
                    send_app_error(&state, &claims.room_id, &queue, err).await;
                }
            }
            other => {
                send_error(
                    &state,
                    &claims.room_id,
                    &queue,
                    ErrorCode::InvalidState,
                    format!("unknown frame {other}"),
                )
                .await;
            }
        }
    }

    if let Some(token) = resume_token {
        state.resume.put(token, claims.player_id.clone()).await;
    }
    state
        .lobby
        .detach_connection(&claims.room_id, &claims.player_id)
        .await;
    writer.abort();
    let _ = writer.await;
    Ok(())
}

async fn handle_batch_input(
    sim: &crate::sim::SimHandle,
    player_id: &str,
    batch: &ClientInputBatch,
    seq: u64,
) {
    for (idx, frame) in batch.frames.iter().enumerate() {
        let tick = batch.start_tick + frame.d;
        sim.submit_input(
            player_id.to_string(),
            tick,
            frame.axis_x,
            frame.jump,
            seq + idx as u64,
        )
        .await;
    }
}

async fn next_seq(state: &Arc<HttpState>, room_id: &str) -> u64 {
    if let Some(room) = state.lobby.room(room_id).await {
        let guard = room.write().await;
        guard.next_seq()
    } else {
        util::now_ms()
    }
}

async fn send_error(
    state: &Arc<HttpState>,
    room_id: &str,
    queue: &ConnectionQueue,
    code: ErrorCode,
    message: String,
) {
    let seq = next_seq(state, room_id).await;
    let payload = WireError::new(code, message);
    let frame = ServerFrame::Error {
        meta: Envelope::new("error", seq, payload),
    };
    queue.push(frame).await;
}

async fn send_app_error(
    state: &Arc<HttpState>,
    room_id: &str,
    queue: &ConnectionQueue,
    err: AppError,
) {
    let seq = next_seq(state, room_id).await;
    let payload = err.wire();
    let frame = ServerFrame::Error {
        meta: Envelope::new("error", seq, payload),
    };
    queue.push(frame).await;
}
