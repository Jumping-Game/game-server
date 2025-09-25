use crate::{
    auth::WsTokenClaims,
    backpressure::BoundedQueue,
    errors::{ErrorCode, ServerError},
    http::HttpState,
    protocol::{
        self, DifficultyConfig, ErrorPayload, InboundMessage, OutboundMessage, PongPayload,
        SessionConfig, SnapshotEvent, StartPayload, WelcomePayload, WorldConfig,
    },
    rate_limit::LeakyBucket,
};
use axum::extract::ws::Message;
use axum::{
    extract::{ws::WebSocketUpgrade, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    Arc,
};
use tokio::sync::{Mutex, Notify};
use tokio::task::JoinHandle;

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
    socket: axum::extract::ws::WebSocket,
    state: Arc<HttpState>,
    claims: WsTokenClaims,
) -> Result<(), ServerError> {
    let runtime = state
        .matchmaker
        .room_runtime(&claims.room_id)
        .ok_or_else(|| ServerError::new(ErrorCode::RoomNotFound, "room not found"))?;
    let seed = state
        .matchmaker
        .room_seed(&claims.room_id)
        .ok_or_else(|| ServerError::new(ErrorCode::RoomNotFound, "room not found"))?;

    let pump = OutboundPump::new(256);
    let seq_counter = Arc::new(AtomicU64::new(1));
    let dropped_counter = Arc::new(AtomicU32::new(0));
    let (mut sender, mut receiver) = socket.split();
    let pump_writer = pump.clone();
    let send_task = tokio::spawn(async move {
        while let Some(pending) = pump_writer.pop().await {
            send_pending(&mut sender, pending).await?;
        }
        Ok::<(), ServerError>(())
    });

    let mut joined = false;
    let mut snapshot_task: Option<JoinHandle<()>> = None;
    let mut input_bucket = LeakyBucket::new(90.0, 90.0);
    let mut batch_bucket = LeakyBucket::new(120.0, 120.0);

    while let Some(result) = receiver.next().await {
        let message = match result {
            Ok(message) => message,
            Err(err) => {
                pump.close();
                if let Some(task) = snapshot_task.take() {
                    task.abort();
                }
                return Err(ServerError::with_source(
                    ErrorCode::Internal,
                    "websocket receive error",
                    err.into(),
                ));
            }
        };

        match message {
            Message::Text(text) => {
                let inbound: InboundMessage = match serde_json::from_str(&text) {
                    Ok(msg) => msg,
                    Err(err) => {
                        queue_error(&pump, &seq_counter, ErrorCode::InvalidState, "invalid json")
                            .await;
                        tracing::debug!(error = %err, "invalid inbound message");
                        continue;
                    }
                };

                if inbound.meta().pv != protocol::SERVER_PV {
                    queue_error(
                        &pump,
                        &seq_counter,
                        ErrorCode::BadVersion,
                        format!(
                            "Server pv={}, client pv={}",
                            protocol::SERVER_PV,
                            inbound.meta().pv
                        ),
                    )
                    .await;
                    break;
                }

                match inbound {
                    InboundMessage::Join { payload, .. } => {
                        if joined {
                            queue_error(
                                &pump,
                                &seq_counter,
                                ErrorCode::InvalidState,
                                "join already processed",
                            )
                            .await;
                            continue;
                        }
                        joined = true;
                        runtime.register_player(&claims.player_id).await;
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
                        pump.push_json(OutboundMessage::new_welcome(
                            next_seq(&seq_counter),
                            welcome,
                        ))
                        .await;

                        let current_tick = runtime.tick().await;
                        let start_payload = StartPayload {
                            start_tick: current_tick,
                            server_tick: current_tick,
                            server_time_ms: current_ts_millis(),
                            tps: state.config.tick_rate_hz,
                        };
                        pump.push_json(OutboundMessage::new_start(
                            next_seq(&seq_counter),
                            start_payload,
                        ))
                        .await;

                        let mut snapshot =
                            runtime.snapshot_for_player(&claims.player_id, true).await;
                        snapshot.stats.dropped_snapshots = dropped_counter.load(Ordering::Relaxed);
                        pump.push_json(OutboundMessage::new_snapshot(
                            next_seq(&seq_counter),
                            snapshot,
                        ))
                        .await;

                        let snapshot_handle = spawn_snapshot_task(
                            runtime.clone(),
                            pump.clone(),
                            seq_counter.clone(),
                            dropped_counter.clone(),
                            claims.player_id.clone(),
                        );
                        snapshot_task = Some(snapshot_handle);

                        tracing::info!(
                            player = %payload.name,
                            room = %claims.room_id,
                            "player joined"
                        );
                    }
                    InboundMessage::Input { ref payload, .. } => {
                        if !joined {
                            queue_error(
                                &pump,
                                &seq_counter,
                                ErrorCode::InvalidState,
                                "must join before sending input",
                            )
                            .await;
                            continue;
                        }
                        if !input_bucket.allow(1.0) {
                            queue_error(
                                &pump,
                                &seq_counter,
                                ErrorCode::RateLimited,
                                "input rate limited",
                            )
                            .await;
                            continue;
                        }
                        let event = crate::room::InputEvent {
                            tick: payload.tick,
                            seq: inbound.meta().seq,
                            axis_x: payload.axis_x,
                            jump: payload.jump,
                        };
                        if let Err(err) = runtime.push_input(&claims.player_id, event).await {
                            queue_error(&pump, &seq_counter, err.code, err.message.clone()).await;
                            if err.code == ErrorCode::CheatDetected {
                                break;
                            }
                        }
                    }
                    InboundMessage::InputBatch { ref payload, .. } => {
                        if !joined {
                            queue_error(
                                &pump,
                                &seq_counter,
                                ErrorCode::InvalidState,
                                "must join before sending input",
                            )
                            .await;
                            continue;
                        }
                        let cost = payload.frames.len().max(1) as f64;
                        if !batch_bucket.allow(cost) {
                            queue_error(
                                &pump,
                                &seq_counter,
                                ErrorCode::RateLimited,
                                "input batch rate limited",
                            )
                            .await;
                            continue;
                        }
                        let mut error: Option<ServerError> = None;
                        for frame in &payload.frames {
                            let event = crate::room::InputEvent {
                                tick: payload.start_tick + frame.d as u64,
                                seq: inbound.meta().seq,
                                axis_x: frame.axis_x,
                                jump: frame.jump,
                            };
                            if let Err(err) = runtime.push_input(&claims.player_id, event).await {
                                error = Some(err);
                                break;
                            }
                        }
                        if let Some(err) = error {
                            queue_error(&pump, &seq_counter, err.code, err.message.clone()).await;
                            if err.code == ErrorCode::CheatDetected {
                                break;
                            }
                        }
                    }
                    InboundMessage::Ping { payload, .. } => {
                        let now = current_ts_millis();
                        let rtt = now.saturating_sub(payload.t0);
                        state.matchmaker.record_latency_sample(rtt);
                        let pong = PongPayload {
                            t0: payload.t0,
                            t1: now,
                        };
                        pump.push_json(OutboundMessage::new_pong(next_seq(&seq_counter), pong))
                            .await;
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
                            queue_error(
                                &pump,
                                &seq_counter,
                                ErrorCode::Unauthorized,
                                "invalid resume token",
                            )
                            .await;
                            continue;
                        }
                        let mut snapshot = runtime.latest_full_snapshot(&payload.player_id).await;
                        snapshot.events.push(SnapshotEvent {
                            kind: "resume".to_string(),
                            x: 0.0,
                            y: 0.0,
                            tick: snapshot.tick,
                        });
                        snapshot.stats.dropped_snapshots = dropped_counter.load(Ordering::Relaxed);
                        pump.push_json(OutboundMessage::new_snapshot(
                            next_seq(&seq_counter),
                            snapshot,
                        ))
                        .await;
                    }
                }
            }
            Message::Binary(_) => {
                queue_error(
                    &pump,
                    &seq_counter,
                    ErrorCode::InvalidState,
                    "binary frames are not supported",
                )
                .await;
            }
            Message::Ping(payload) => {
                pump.push_control(Message::Pong(payload)).await;
            }
            Message::Pong(_) => {}
            Message::Close(_) => {
                break;
            }
        }
    }

    pump.close();
    if let Some(task) = snapshot_task {
        task.abort();
    }

    match send_task.await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(err)) => Err(err),
        Err(_) => Ok(()),
    }
}

#[derive(Clone)]
struct OutboundPump {
    inner: Arc<OutboundPumpInner>,
}

struct OutboundPumpInner {
    queue: Mutex<BoundedQueue<PendingMessage>>,
    notify: Notify,
    closed: AtomicBool,
}

enum PendingMessage {
    Json(OutboundMessage),
    Control(Message),
}

impl OutboundPump {
    fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(OutboundPumpInner {
                queue: Mutex::new(BoundedQueue::new(capacity)),
                notify: Notify::new(),
                closed: AtomicBool::new(false),
            }),
        }
    }

    async fn push_json(&self, message: OutboundMessage) -> bool {
        let mut queue = self.inner.queue.lock().await;
        let before = queue.dropped();
        queue.push(PendingMessage::Json(message));
        let after = queue.dropped();
        drop(queue);
        self.inner.notify.notify_one();
        after > before
    }

    async fn push_control(&self, message: Message) -> bool {
        let mut queue = self.inner.queue.lock().await;
        let before = queue.dropped();
        queue.push(PendingMessage::Control(message));
        let after = queue.dropped();
        drop(queue);
        self.inner.notify.notify_one();
        after > before
    }

    async fn pop(&self) -> Option<PendingMessage> {
        loop {
            let mut queue = self.inner.queue.lock().await;
            if let Some(item) = queue.pop() {
                return Some(item);
            }
            if self.inner.closed.load(Ordering::Relaxed) {
                return None;
            }
            let notified = self.inner.notify.notified();
            drop(queue);
            notified.await;
        }
    }

    fn close(&self) {
        if !self.inner.closed.swap(true, Ordering::Relaxed) {
            self.inner.notify.notify_waiters();
        }
    }
}

async fn send_pending(
    sink: &mut futures_util::stream::SplitSink<axum::extract::ws::WebSocket, Message>,
    message: PendingMessage,
) -> Result<(), ServerError> {
    match message {
        PendingMessage::Json(message) => {
            let text = serde_json::to_string(&message).map_err(|err| {
                ServerError::with_source(ErrorCode::Internal, "serialization error", err.into())
            })?;
            sink.send(Message::Text(text)).await.map_err(|err| {
                ServerError::with_source(ErrorCode::Internal, "failed to send", err.into())
            })
        }
        PendingMessage::Control(message) => sink.send(message).await.map_err(|err| {
            ServerError::with_source(ErrorCode::Internal, "failed to send", err.into())
        }),
    }
}

fn next_seq(counter: &AtomicU64) -> u64 {
    counter.fetch_add(1, Ordering::Relaxed)
}

async fn queue_error(
    pump: &OutboundPump,
    seq_counter: &AtomicU64,
    code: ErrorCode,
    message: impl Into<String>,
) {
    let payload = ErrorPayload {
        code,
        message: message.into(),
    };
    let _ = pump
        .push_json(OutboundMessage::new_error(next_seq(seq_counter), payload))
        .await;
}

fn spawn_snapshot_task(
    runtime: Arc<crate::room::RoomRuntime>,
    pump: OutboundPump,
    seq_counter: Arc<AtomicU64>,
    dropped_counter: Arc<AtomicU32>,
    player_id: String,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut rx = runtime.subscribe();
        while rx.changed().await.is_ok() {
            let signal = rx.borrow().clone();
            if signal.tick == 0 {
                continue;
            }
            let mut snapshot = runtime.snapshot_for_player(&player_id, signal.full).await;
            snapshot.stats.dropped_snapshots = dropped_counter.load(Ordering::Relaxed);
            if pump
                .push_json(OutboundMessage::new_snapshot(
                    next_seq(&seq_counter),
                    snapshot,
                ))
                .await
            {
                let prev = dropped_counter.fetch_add(1, Ordering::Relaxed);
                if prev == 0 {
                    let _ = pump
                        .push_json(OutboundMessage::new_error(
                            next_seq(&seq_counter),
                            ErrorPayload {
                                code: ErrorCode::SlowConsumer,
                                message: "client is too slow to consume snapshots".to_string(),
                            },
                        ))
                        .await;
                }
            }
        }
    })
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
