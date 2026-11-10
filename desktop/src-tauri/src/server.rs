//! Optional local web server: the same UI and a small JSON API, with an optional
//! HTTP Basic password. Listens on 127.0.0.1, or on all interfaces when LAN access
//! is enabled.

use std::collections::HashSet;
use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use base64::Engine as _;
use futures_util::stream::{self, Stream};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::AppHandle;
use tokio::sync::{broadcast, watch};

use crate::bulbs::Patch;
use crate::core::{self, SceneInput, SettingsUpdate};
use crate::secrets;
use crate::settings::WebSettings;

const CSP: &str = "default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; \
                   connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'";

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebStatus {
    pub enabled: bool,
    pub running: bool,
    pub lan: bool,
    pub has_password: bool,
    pub local_url: Option<String>,
    pub lan_url: Option<String>,
    pub error: Option<String>,
}

pub struct WebServer {
    stop: tokio::sync::Mutex<Option<watch::Sender<bool>>>,
    status: Mutex<WebStatus>,
}

#[derive(Clone)]
struct ServerState {
    app: AppHandle,
    password_hash: Option<Arc<str>>,
    /// SHA-256 of Authorization headers that already passed the (slow) Argon2 check.
    verified: Arc<Mutex<HashSet<[u8; 32]>>>,
    stop: watch::Receiver<bool>,
}

impl WebServer {
    pub fn new() -> Self {
        Self { stop: tokio::sync::Mutex::new(None), status: Mutex::new(WebStatus::default()) }
    }

    pub fn status(&self) -> WebStatus {
        self.status.lock().unwrap().clone()
    }

    /// Stop the running server (if any) and start one for `cfg` (if enabled).
    pub async fn apply(&self, app: AppHandle, cfg: WebSettings, bulb_ip: Option<String>) {
        let mut stop = self.stop.lock().await;
        if let Some(previous) = stop.take() {
            let _ = previous.send(true);
        }
        let mut status = WebStatus {
            enabled: cfg.enabled,
            lan: cfg.lan,
            has_password: cfg.password_hash.is_some(),
            ..Default::default()
        };
        if cfg.enabled {
            let ip = if cfg.lan { Ipv4Addr::UNSPECIFIED } else { Ipv4Addr::LOCALHOST };
            let addr = SocketAddr::new(IpAddr::V4(ip), cfg.port);
            let mut bound = Err(std::io::Error::other("not tried"));
            // The previous listener may need a moment to release the port.
            for _ in 0..15 {
                bound = tokio::net::TcpListener::bind(addr).await;
                if bound.is_ok() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            match bound {
                Ok(listener) => {
                    let (tx, rx) = watch::channel(false);
                    let state = ServerState {
                        app: app.clone(),
                        password_hash: cfg.password_hash.as_deref().map(Arc::from),
                        verified: Arc::default(),
                        stop: rx.clone(),
                    };
                    let mut shutdown = rx;
                    tauri::async_runtime::spawn(async move {
                        let _ = axum::serve(listener, router(state))
                            .with_graceful_shutdown(async move {
                                let _ = shutdown.wait_for(|stopped| *stopped).await;
                            })
                            .await;
                    });
                    *stop = Some(tx);
                    status.running = true;
                    status.local_url = Some(format!("http://localhost:{}", cfg.port));
                    if cfg.lan {
                        status.lan_url = lan_address(bulb_ip).map(|ip| format!("http://{ip}:{}", cfg.port));
                    }
                }
                Err(e) => status.error = Some(format!("Port {} is not available ({e})", cfg.port)),
            }
        }
        *self.status.lock().unwrap() = status;
        core::state(&app).bulbs.notify();
    }
}

/// The local interface address that routes toward the bulbs (no packet is sent).
fn lan_address(toward: Option<String>) -> Option<IpAddr> {
    let target: IpAddr = toward.and_then(|t| t.parse().ok()).unwrap_or(IpAddr::V4(Ipv4Addr::new(192, 168, 0, 1)));
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect((target, 9)).ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

fn router(state: ServerState) -> Router {
    Router::new()
        .route("/api/snapshot", get(snapshot))
        .route("/api/events", get(events))
        .route("/api/command", post(command))
        .route("/api/refresh", post(refresh))
        .route("/api/scenes/apply", post(apply_scene))
        .route("/api/scenes", post(save_scene))
        .route("/api/scenes/{name}", delete(delete_scene))
        .route("/api/settings", get(get_settings).put(put_settings))
        .fallback(get(static_file))
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state)
}

/// Host check (blocks DNS-rebinding pages from reaching the API), optional
/// password, and security headers.
async fn guard(State(s): State<ServerState>, req: Request, next: Next) -> Response {
    let host = req.headers().get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("");
    if !host_allowed(host) {
        return (StatusCode::FORBIDDEN, "Open this page by IP address or localhost.").into_response();
    }
    if let Some(hash) = &s.password_hash {
        let auth = req.headers().get(header::AUTHORIZATION).and_then(|h| h.to_str().ok()).unwrap_or("");
        if !authorized(&s, hash, auth) {
            if !auth.is_empty() {
                tokio::time::sleep(Duration::from_millis(500)).await; // slow down guessing
            }
            return (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Basic realm=\"Tapo Desktop\", charset=\"UTF-8\"")],
                "Password required",
            )
                .into_response();
        }
    }
    let api = req.uri().path().starts_with("/api/");
    let mut res = next.run(req).await;
    let headers = res.headers_mut();
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    if api {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    res
}

fn host_allowed(host: &str) -> bool {
    let name = match host.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or(""),
        None => host.rsplit_once(':').map_or(host, |(h, _)| h),
    };
    name.eq_ignore_ascii_case("localhost") || name.parse::<IpAddr>().is_ok()
}

fn authorized(s: &ServerState, hash: &str, auth: &str) -> bool {
    let Some(encoded) = auth.strip_prefix("Basic ") else { return false };
    let digest: [u8; 32] = Sha256::digest(auth.as_bytes()).into();
    if s.verified.lock().unwrap().contains(&digest) {
        return true;
    }
    let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(encoded.trim()) else { return false };
    let Ok(text) = String::from_utf8(decoded) else { return false };
    let password = text.split_once(':').map_or("", |(_, p)| p);
    let ok = secrets::verify_web_password(password, hash);
    if ok {
        s.verified.lock().unwrap().insert(digest);
    }
    ok
}

type ApiError = (StatusCode, String);

fn bad_request(message: String) -> ApiError {
    (StatusCode::BAD_REQUEST, message)
}

async fn snapshot(State(s): State<ServerState>) -> Json<core::Snapshot> {
    Json(core::snapshot(&s.app))
}

async fn events(State(s): State<ServerState>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = core::state(&s.app).bulbs.subscribe();
    let stream = stream::unfold((rx, s, true), |(mut rx, s, first)| async move {
        if !first {
            let mut stop = s.stop.clone();
            if *stop.borrow() {
                return None;
            }
            tokio::select! {
                r = rx.recv() => if matches!(r, Err(broadcast::error::RecvError::Closed)) { return None; },
                _ = stop.changed() => return None,
            }
            // Coalesce bursts (slider drags) into one snapshot.
            tokio::time::sleep(Duration::from_millis(30)).await;
            while matches!(rx.try_recv(), Ok(_) | Err(broadcast::error::TryRecvError::Lagged(_))) {}
        }
        let json = serde_json::to_string(&core::snapshot(&s.app)).ok()?;
        Some((Ok(Event::default().event("snapshot").data(json)), (rx, s, false)))
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[derive(Deserialize)]
struct CommandBody {
    #[serde(default)]
    targets: Vec<String>,
    patch: Patch,
}

async fn command(State(s): State<ServerState>, Json(body): Json<CommandBody>) -> StatusCode {
    core::command(&s.app, &body.targets, body.patch);
    StatusCode::NO_CONTENT
}

async fn refresh(State(s): State<ServerState>) -> StatusCode {
    core::refresh(&s.app);
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
struct ApplySceneBody {
    name: String,
    #[serde(default)]
    targets: Vec<String>,
}

async fn apply_scene(State(s): State<ServerState>, Json(body): Json<ApplySceneBody>) -> Result<StatusCode, ApiError> {
    core::apply_scene(&s.app, &body.name, &body.targets).map_err(bad_request)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct SaveSceneBody {
    name: String,
    scene: SceneInput,
}

async fn save_scene(State(s): State<ServerState>, Json(body): Json<SaveSceneBody>) -> Result<StatusCode, ApiError> {
    core::save_scene(&s.app, &body.name, body.scene).map_err(bad_request)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_scene(State(s): State<ServerState>, Path(name): Path<String>) -> Result<StatusCode, ApiError> {
    core::delete_scene(&s.app, &name).map_err(bad_request)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_settings(State(s): State<ServerState>) -> Json<core::SettingsView> {
    Json(core::settings_view(&s.app, true))
}

async fn put_settings(
    State(s): State<ServerState>,
    Json(update): Json<SettingsUpdate>,
) -> Result<Json<core::SettingsView>, ApiError> {
    core::save_settings(&s.app, update, true).await.map(Json).map_err(bad_request)
}

async fn static_file(State(s): State<ServerState>, uri: Uri) -> Response {
    let resolver = s.app.asset_resolver();
    let asset = resolver.get(uri.path().to_string()).or_else(|| resolver.get("index.html".into()));
    match asset {
        Some(asset) => Response::builder()
            .header(header::CONTENT_TYPE, asset.mime_type)
            .header(header::CONTENT_SECURITY_POLICY, CSP)
            .body(Body::from(asset.bytes))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()),
        None => (StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::host_allowed;

    #[test]
    fn host_check_accepts_ip_literals_and_localhost_only() {
        for ok in ["localhost:4530", "127.0.0.1:4530", "192.168.1.20:4530", "[::1]:4530", "localhost"] {
            assert!(host_allowed(ok), "{ok}");
        }
        for bad in ["evil.example:4530", "", "attacker.localhost.example", "rebind.me"] {
            assert!(!host_allowed(bad), "{bad}");
        }
    }
}
