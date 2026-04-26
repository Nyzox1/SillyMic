use std::{net::SocketAddr, sync::Arc, time::Duration};

use anyhow::{anyhow, Context};
use axum::{
    extract::{
        ws::{Message, WebSocket},
        Query, State, WebSocketUpgrade,
    },
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use futures::{SinkExt, StreamExt};
use rand::Rng;
use serde::{Deserialize, Serialize};
use tokio::{
    net::TcpListener,
    sync::mpsc,
    time::{interval, timeout},
};
use tracing::{error, info, warn};
use uuid::Uuid;
use webrtc::{
    ice_transport::ice_candidate::RTCIceCandidateInit,
    peer_connection::{
        peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription,
    },
};

use crate::{
    audio::start_audio_bridge,
    session::{SessionManager, SessionStatus, SessionView},
    signal::SignalMessage,
    webrtc_engine::WebRtcEngine,
};

const SIGNAL_FIRST_MESSAGE_TIMEOUT: Duration = Duration::from_secs(10);
const SESSION_TTL: Duration = Duration::from_secs(60);
const MAX_SIGNAL_PAYLOAD: usize = 128 * 1024;

#[derive(Debug, Clone)]
pub struct HostConfig {
    pub port: u16,
    pub pin: String,
    pub input_selector: Option<String>,
}

#[derive(Clone)]
struct AppState {
    sessions: Arc<SessionManager>,
    webrtc: Arc<WebRtcEngine>,
}

pub async fn run_host(config: HostConfig) -> anyhow::Result<()> {
    let sessions = Arc::new(SessionManager::new(SESSION_TTL));
    let session = sessions.create_session(config.pin.clone()).await;

    let webrtc = Arc::new(WebRtcEngine::new()?);
    let _audio = start_audio_bridge(config.input_selector.as_deref(), webrtc.track())
        .context("Could not start audio capture")?;

    let app_state = Arc::new(AppState { sessions, webrtc });
    spawn_session_cleanup(Arc::clone(&app_state));

    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/session/create", post(create_session_handler))
        .route("/session/status", get(session_status_handler))
        .route("/signal", get(signal_handler))
        .with_state(Arc::clone(&app_state));

    let listener = TcpListener::bind(("0.0.0.0", config.port))
        .await
        .with_context(|| format!("Could not bind on port {}", config.port))?;

    let lan_ip = local_ip_address::local_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string());

    info!("SillyMic host started");
    info!("LAN endpoint: ws://{}:{}/signal", lan_ip, config.port);
    info!(
        "HTTP endpoints: http://{}:{}/health, /session/create, /session/status",
        lan_ip, config.port
    );
    info!(
        "Session created: id={}, code={}",
        session.id, session.session_code
    );

    let server = axum::serve(listener, app.into_make_service()).with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
        info!("Shutdown requested");
    });
    server.await.context("Server stopped with an error")
}

fn spawn_session_cleanup(app_state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(5));
        loop {
            tick.tick().await;
            app_state.sessions.cleanup_expired().await;
        }
    });
}

#[derive(Serialize)]
struct HealthResponse<'a> {
    status: &'a str,
    version: &'a str,
    #[serde(rename = "hasSession")]
    has_session: bool,
}

async fn health_handler(State(state): State<Arc<AppState>>) -> Json<HealthResponse<'static>> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        has_session: state.sessions.has_session().await,
    })
}

#[derive(Deserialize)]
struct SessionCreateRequest {
    pin: Option<String>,
}

#[derive(Serialize)]
struct SessionCreateResponse {
    #[serde(rename = "sessionId")]
    session_id: Uuid,
    #[serde(rename = "sessionCode")]
    session_code: String,
    #[serde(rename = "expiresInSeconds")]
    expires_in_seconds: u64,
    status: SessionStatus,
}

#[derive(Serialize)]
struct ApiError {
    code: &'static str,
    message: String,
}

async fn create_session_handler(
    State(state): State<Arc<AppState>>,
    maybe_json: Result<Json<SessionCreateRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<SessionCreateResponse>, (StatusCode, Json<ApiError>)> {
    let pin = match maybe_json {
        Ok(Json(req)) => req.pin.unwrap_or_else(generate_pin),
        Err(_) => generate_pin(),
    };

    if pin.len() != 6 || !pin.chars().all(|c| c.is_ascii_digit()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "BAD_PIN",
                message: "PIN must contain exactly 6 digits".to_string(),
            }),
        ));
    }

    let created = state.sessions.create_session(pin).await;
    Ok(Json(SessionCreateResponse {
        session_id: created.id,
        session_code: created.session_code,
        expires_in_seconds: created.expires_in_seconds,
        status: created.status,
    }))
}

fn generate_pin() -> String {
    let mut rng = rand::thread_rng();
    format!("{:06}", rng.gen_range(0..1_000_000))
}

#[derive(Deserialize)]
struct SessionStatusQuery {
    id: Uuid,
}

async fn session_status_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SessionStatusQuery>,
) -> Result<Json<SessionView>, (StatusCode, Json<ApiError>)> {
    match state.sessions.by_id(query.id).await {
        Some(view) => Ok(Json(view)),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ApiError {
                code: "SESSION_NOT_FOUND",
                message: "Session not found".to_string(),
            }),
        )),
    }
}

async fn signal_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_signal_socket(socket, state))
}

struct ActiveConnectionGuard {
    sessions: Arc<SessionManager>,
}

impl ActiveConnectionGuard {
    fn try_new(sessions: Arc<SessionManager>) -> Option<Self> {
        if sessions.try_acquire_connection() {
            Some(Self { sessions })
        } else {
            None
        }
    }
}

impl Drop for ActiveConnectionGuard {
    fn drop(&mut self) {
        self.sessions.release_connection();
    }
}

async fn handle_signal_socket(socket: WebSocket, state: Arc<AppState>) {
    let Some(_guard) = ActiveConnectionGuard::try_new(Arc::clone(&state.sessions)) else {
        let _ = reject_socket(socket, "BUSY", "Another client is already connected").await;
        return;
    };

    let (mut ws_tx, mut ws_rx) = socket.split();
    let (signal_out_tx, mut signal_out_rx) = mpsc::unbounded_channel::<SignalMessage>();

    let writer_task = tokio::spawn(async move {
        while let Some(msg) = signal_out_rx.recv().await {
            let payload = match serde_json::to_string(&msg) {
                Ok(s) => s,
                Err(err) => {
                    error!("Could not encode outgoing signal message: {err}");
                    continue;
                }
            };
            if ws_tx.send(Message::Text(payload)).await.is_err() {
                break;
            }
        }
    });

    let first = match timeout(SIGNAL_FIRST_MESSAGE_TIMEOUT, ws_rx.next()).await {
        Ok(Some(Ok(msg))) => msg,
        _ => {
            let _ = signal_out_tx.send(SignalMessage::error(
                "HELLO_TIMEOUT",
                "Expected hello as first message",
            ));
            drop(signal_out_tx);
            let _ = writer_task.await;
            return;
        }
    };

    let parsed_first = match parse_signal_message(first) {
        Ok(msg) => msg,
        Err(err) => {
            let _ = signal_out_tx.send(SignalMessage::error("BAD_SIGNAL", err.to_string()));
            drop(signal_out_tx);
            let _ = writer_task.await;
            return;
        }
    };
    let SignalMessage::Hello {
        device_name,
        app_version,
        session_code,
    } = parsed_first
    else {
        let _ = signal_out_tx.send(SignalMessage::error(
            "HELLO_REQUIRED",
            "First websocket message must be hello",
        ));
        drop(signal_out_tx);
        let _ = writer_task.await;
        return;
    };

    info!("Mobile hello from '{device_name}' ({app_version})");

    let Some(session_id) = state.sessions.validate_code(&session_code).await else {
        let _ = signal_out_tx.send(SignalMessage::error(
            "SESSION_INVALID",
            "Invalid or expired session code",
        ));
        drop(signal_out_tx);
        let _ = writer_task.await;
        return;
    };

    state
        .sessions
        .set_status(session_id, SessionStatus::Connecting)
        .await;

    let peer = match state.webrtc.create_peer(signal_out_tx.clone()).await {
        Ok(pc) => pc,
        Err(err) => {
            state
                .sessions
                .set_error(session_id, "PEER_INIT_FAILED", &err.to_string())
                .await;
            let _ = signal_out_tx.send(SignalMessage::error(
                "PEER_INIT_FAILED",
                "Could not create WebRTC peer",
            ));
            drop(signal_out_tx);
            let _ = writer_task.await;
            return;
        }
    };

    {
        let sessions = Arc::clone(&state.sessions);
        peer.on_peer_connection_state_change(Box::new(move |pc_state: RTCPeerConnectionState| {
            let sessions = Arc::clone(&sessions);
            Box::pin(async move {
                match pc_state {
                    RTCPeerConnectionState::Connected => {
                        sessions
                            .set_status(session_id, SessionStatus::Streaming)
                            .await;
                    }
                    RTCPeerConnectionState::Failed | RTCPeerConnectionState::Disconnected => {
                        sessions
                            .set_error(session_id, "PEER_DISCONNECTED", "Peer disconnected")
                            .await;
                    }
                    RTCPeerConnectionState::Closed => {
                        sessions
                            .set_status(session_id, SessionStatus::Waiting)
                            .await;
                    }
                    _ => {}
                }
            })
        }));
    }

    let _ = signal_out_tx.send(SignalMessage::Ready {
        session_id: session_id.to_string(),
    });

    while let Some(next) = ws_rx.next().await {
        let message = match next {
            Ok(m) => m,
            Err(err) => {
                warn!("Websocket receive error: {err}");
                break;
            }
        };

        let signal = match parse_signal_message(message) {
            Ok(s) => s,
            Err(err) => {
                let _ = signal_out_tx.send(SignalMessage::error("BAD_SIGNAL", err.to_string()));
                continue;
            }
        };

        state.sessions.touch(session_id).await;

        match signal {
            SignalMessage::Offer { sdp } => {
                if let Err(err) = handle_offer(&peer, &signal_out_tx, sdp).await {
                    state
                        .sessions
                        .set_error(session_id, "OFFER_FAILED", &err.to_string())
                        .await;
                    let _ = signal_out_tx.send(SignalMessage::error(
                        "OFFER_FAILED",
                        "Could not process SDP offer",
                    ));
                }
            }
            SignalMessage::IceCandidate {
                candidate,
                sdp_mid,
                sdp_mline_index,
            } => {
                let ice = RTCIceCandidateInit {
                    candidate,
                    sdp_mid,
                    sdp_mline_index,
                    username_fragment: None,
                };
                if let Err(err) = peer.add_ice_candidate(ice).await {
                    warn!("Could not add remote ICE candidate: {err}");
                }
            }
            SignalMessage::Ready { .. }
            | SignalMessage::Hello { .. }
            | SignalMessage::Answer { .. }
            | SignalMessage::Error { .. } => {}
        }
    }

    let _ = peer.close().await;
    state
        .sessions
        .set_status(session_id, SessionStatus::Waiting)
        .await;
    drop(signal_out_tx);
    let _ = writer_task.await;
}

async fn reject_socket(mut socket: WebSocket, code: &str, message: &str) -> anyhow::Result<()> {
    let payload = serde_json::to_string(&SignalMessage::error(code, message))
        .context("Could not serialize rejection message")?;
    socket
        .send(Message::Text(payload))
        .await
        .context("Could not send websocket rejection")?;
    socket.close().await.context("Could not close websocket")?;
    Ok(())
}

async fn handle_offer(
    peer: &Arc<webrtc::peer_connection::RTCPeerConnection>,
    signal_out_tx: &mpsc::UnboundedSender<SignalMessage>,
    sdp: String,
) -> anyhow::Result<()> {
    let offer = RTCSessionDescription::offer(sdp).context("Invalid offer SDP")?;
    peer.set_remote_description(offer)
        .await
        .context("Could not set remote description")?;

    let mut gather_complete = peer.gathering_complete_promise().await;
    let answer = peer
        .create_answer(None)
        .await
        .context("Could not create answer")?;

    peer.set_local_description(answer)
        .await
        .context("Could not set local description")?;
    let _ = gather_complete.recv().await;

    let local = peer
        .local_description()
        .await
        .ok_or_else(|| anyhow!("Missing local description"))?;

    let _ = signal_out_tx.send(SignalMessage::Answer { sdp: local.sdp });
    Ok(())
}

fn parse_signal_message(message: Message) -> anyhow::Result<SignalMessage> {
    match message {
        Message::Text(text) => {
            if text.len() > MAX_SIGNAL_PAYLOAD {
                return Err(anyhow!("Signal payload too large"));
            }
            serde_json::from_str::<SignalMessage>(&text).context("Invalid signal payload")
        }
        Message::Binary(bin) => {
            if bin.len() > MAX_SIGNAL_PAYLOAD {
                return Err(anyhow!("Signal payload too large"));
            }
            serde_json::from_slice::<SignalMessage>(&bin).context("Invalid binary signal payload")
        }
        Message::Ping(_) | Message::Pong(_) => Err(anyhow!("Control frame is not a signal")),
        Message::Close(_) => Err(anyhow!("Websocket closed")),
    }
}

#[allow(dead_code)]
fn _socket_addr_from_port(port: u16) -> SocketAddr {
    SocketAddr::from(([0, 0, 0, 0], port))
}
