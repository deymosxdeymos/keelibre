use std::{
    convert::Infallible,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Result;
use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    middleware::{Next, from_fn},
    response::{Html, IntoResponse, Sse, sse::Event},
    routing::{get, post},
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::broadcast;
use tokio_stream::{StreamExt, wrappers::BroadcastStream};

use crate::{
    audio::AudioEngine,
    catalog,
    config::Config,
    keymap::KeyGroup,
    sound::{self, Phase, Profile},
};

#[derive(Clone, Copy, Debug, Serialize)]
pub struct KeyEvent {
    pub code: u16,
    pub phase: u8,
}

#[derive(Clone)]
pub struct WebState {
    engine: Arc<AudioEngine>,
    config_path: Arc<PathBuf>,
    events: broadcast::Sender<KeyEvent>,
    updates: Arc<Mutex<()>>,
}

impl WebState {
    #[must_use]
    pub fn new(engine: Arc<AudioEngine>, config_path: PathBuf) -> Self {
        let (events, _) = broadcast::channel(128);
        Self {
            engine,
            config_path: Arc::new(config_path),
            events,
            updates: Arc::new(Mutex::new(())),
        }
    }

    pub fn publish(&self, event: KeyEvent) {
        let _ = self.events.send(event);
    }

    async fn change(
        &self,
        reload_profile: bool,
        change: impl FnOnce(&mut Config) -> Result<(), ApiError> + Send + 'static,
    ) -> Result<Config, ApiError> {
        let state = self.clone();
        tokio::task::spawn_blocking(move || {
            let _guard = state.updates.lock();
            let old = state.engine.settings();
            let mut config = old.clone();
            change(&mut config)?;
            let profile = if reload_profile
                || config.profile != old.profile
                || config.sounds_dir != old.sounds_dir
            {
                Some(Profile::load(&config.sounds_dir.join(&config.profile))?)
            } else {
                None
            };
            config.save(&state.config_path)?;
            if config.auto_start != old.auto_start {
                set_auto_start(config.auto_start);
            }
            state.engine.apply(config.clone(), profile);
            Ok(config)
        })
        .await?
    }

    pub async fn reload(&self) -> Result<()> {
        let state = self.clone();
        tokio::task::spawn_blocking(move || {
            let _guard = state.updates.lock();
            let config = Config::load(&state.config_path)?;
            if config.ui_port != state.engine.settings().ui_port {
                anyhow::bail!("ui_port requires a service restart");
            }
            let profile = Profile::load(&config.sounds_dir.join(&config.profile))?;
            state.engine.apply(config, Some(profile));
            Ok(())
        })
        .await?
    }
}

pub async fn serve(listener: tokio::net::TcpListener, state: WebState) -> Result<()> {
    let port = listener.local_addr()?.port();
    let router = Router::new()
        .route("/", get(index))
        .route("/api/status", get(status))
        .route("/api/settings", get(settings).post(update_settings))
        .route("/api/profiles", get(profiles))
        .route("/api/select", post(select_profile))
        .route("/api/preview", post(preview))
        .route("/api/preview-overlay", post(preview_overlay))
        .route("/api/favorites", post(favorite))
        .route("/api/toggle-mute", post(toggle_mute))
        .route("/api/events", get(events))
        .layer(from_fn(
            move |request: axum::extract::Request, next: Next| async move {
                if !local_request(request.headers(), port) {
                    return StatusCode::FORBIDDEN.into_response();
                }
                next.run(request).await
            },
        ))
        .with_state(state);
    tracing::info!(url = %format!("http://{}", listener.local_addr()?), "control panel ready");
    axum::serve(listener, router).await?;
    Ok(())
}

fn local_request(headers: &HeaderMap, port: u16) -> bool {
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    (host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}"))
        && headers.get(header::ORIGIN).is_none_or(|origin| {
            origin
                .to_str()
                .is_ok_and(|origin| origin == format!("http://{host}"))
        })
}

async fn index() -> Html<&'static str> {
    Html(include_str!("../assets/panel.html"))
}

async fn status(State(state): State<WebState>) -> Json<Value> {
    let settings = state.engine.settings();
    Json(
        json!({ "profile": settings.profile, "muted": state.engine.muted(), "enabled": settings.enabled,
            "auto_start_supported": cfg!(target_os = "linux") }),
    )
}

async fn settings(State(state): State<WebState>) -> Json<Config> {
    Json(state.engine.settings())
}

async fn update_settings(
    State(state): State<WebState>,
    Json(values): Json<Value>,
) -> Result<Json<Config>, ApiError> {
    let object = values
        .as_object()
        .ok_or_else(|| ApiError::bad_request("expected a JSON object"))?
        .clone();
    Ok(Json(
        state
            .change(false, move |config| {
                for (key, value) in object {
                    if key == "auto_start" && !cfg!(target_os = "linux") {
                        return Err(ApiError::bad_request(
                            "configure startup in OS Login Items or Startup Apps",
                        ));
                    }
                    if matches!(key.as_str(), "profile" | "sounds_dir" | "ui_port") {
                        return Err(ApiError::bad_request(
                            "use the profile selector or config file",
                        ));
                    }
                    let value = value
                        .as_str()
                        .map_or_else(|| value.to_string(), ToOwned::to_owned);
                    config.set(&key, &value)?;
                }
                Ok(())
            })
            .await?,
    ))
}

#[cfg(target_os = "linux")]
fn set_auto_start(enabled: bool) {
    let action = if enabled { "enable" } else { "disable" };
    match std::process::Command::new("systemctl")
        .args(["--user", action, "keebyd.service", "keebyd-ui.service"])
        .status()
    {
        Ok(status) if status.success() => {}
        Ok(status) => tracing::warn!(%status, "could not update launch-at-login setting"),
        Err(error) => tracing::warn!(%error, "could not update launch-at-login setting"),
    }
}

#[cfg(not(target_os = "linux"))]
const fn set_auto_start(_enabled: bool) {}

#[derive(Serialize)]
struct Profiles {
    profiles: Vec<ProfileItem>,
}

#[derive(Serialize)]
struct ProfileItem {
    name: String,
    display: String,
    brand: String,
    #[serde(rename = "type")]
    kind: String,
    color: String,
    contributor: String,
    favorite: bool,
    norm: f32,
}

async fn profiles(State(state): State<WebState>) -> Result<Json<Profiles>, ApiError> {
    let settings = state.engine.settings();
    let favorites = settings.favorites.split(',').collect::<Vec<_>>();
    let mut items = std::fs::read_dir(&settings.sounds_dir)?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            if name == "_shared" || name == "ticks" || !contains_wav(&entry.path()) {
                return None;
            }
            let meta = catalog::find(&name);
            Some(ProfileItem {
                display: meta.map_or_else(|| name.clone(), |item| item.display.into()),
                brand: meta.map_or_else(|| "Other".into(), |item| item.brand.into()),
                kind: meta.map_or_else(|| "Custom pack".into(), |item| item.kind.into()),
                color: meta.map_or_else(|| "#777777".into(), |item| item.color.into()),
                contributor: meta.map_or(String::new(), |item| item.contributor.into()),
                norm: meta.map_or(1.0, |item| item.norm),
                favorite: favorites.contains(&name.as_str()),
                name,
            })
        })
        .collect::<Vec<_>>();
    items.sort_by(|a, b| a.brand.cmp(&b.brand).then(a.display.cmp(&b.display)));
    Ok(Json(Profiles { profiles: items }))
}

#[derive(Deserialize)]
struct OverlayQuery {
    kind: String,
}

async fn preview_overlay(
    State(state): State<WebState>,
    Query(query): Query<OverlayQuery>,
) -> Result<Json<Value>, ApiError> {
    match query.kind.as_str() {
        "mouse" => state.engine.preview_mouse(),
        "enter" => state.engine.play_enter_overlay(),
        _ => return Err(ApiError::bad_request("unknown overlay sound")),
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct NameQuery {
    name: String,
}

async fn select_profile(
    State(state): State<WebState>,
    Query(query): Query<NameQuery>,
) -> Result<Json<Value>, ApiError> {
    validate_component(&query.name)?;
    state
        .change(true, move |settings| {
            settings.profile = query.name;
            Ok(())
        })
        .await?;
    Ok(Json(json!({ "ok": true })))
}

async fn preview(
    State(state): State<WebState>,
    Query(query): Query<NameQuery>,
) -> Result<Json<Value>, ApiError> {
    validate_component(&query.name)?;
    let engine = Arc::clone(&state.engine);
    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        let config = engine.settings();
        if query.name == config.profile {
            if let Some(sample) = engine.profile().and_then(|profile| {
                profile
                    .variations(KeyGroup::Alpha, Phase::Down)
                    .and_then(|set| set.first().cloned())
            }) {
                engine.preview(sample);
            }
        } else {
            engine.preview(sound::preview_sample(&config.sounds_dir.join(query.name))?);
        }
        Ok(())
    })
    .await??;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct FavoriteQuery {
    name: String,
    on: u8,
}

async fn favorite(
    State(state): State<WebState>,
    Query(query): Query<FavoriteQuery>,
) -> Result<Json<Value>, ApiError> {
    validate_component(&query.name)?;
    state
        .change(false, move |settings| {
            let mut favorites = settings
                .favorites
                .split(',')
                .filter(|item| !item.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>();
            favorites.retain(|item| item != &query.name);
            if query.on != 0 {
                favorites.push(query.name);
            }
            favorites.sort();
            favorites.dedup();
            settings.favorites = favorites.join(",");
            Ok(())
        })
        .await?;
    Ok(Json(json!({ "ok": true })))
}

async fn toggle_mute(State(state): State<WebState>) -> Json<Value> {
    Json(json!({ "muted": state.engine.toggle_muted() }))
}

async fn events(
    State(state): State<WebState>,
) -> Sse<impl futures_core::Stream<Item = Result<Event, Infallible>>> {
    let stream = BroadcastStream::new(state.events.subscribe()).filter_map(|result| {
        result.ok().map(|event| {
            Ok(Event::default()
                .json_data(event)
                .expect("key event serializes"))
        })
    });
    Sse::new(stream)
}

fn contains_wav(path: &Path) -> bool {
    std::fs::read_dir(path).is_ok_and(|entries| {
        entries.filter_map(Result::ok).any(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("wav"))
        })
    })
}

fn validate_component(value: &str) -> Result<(), ApiError> {
    if value.is_empty() || value == "." || value == ".." || value.contains(['/', '\\']) {
        return Err(ApiError::bad_request("invalid path component"));
    }
    Ok(())
}

struct ApiError(StatusCode, anyhow::Error);
impl ApiError {
    fn bad_request(message: &'static str) -> Self {
        Self(StatusCode::BAD_REQUEST, anyhow::anyhow!(message))
    }
}
impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(error: E) -> Self {
        Self(StatusCode::INTERNAL_SERVER_ERROR, error.into())
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        tracing::warn!(error = %self.1, "API request failed");
        (self.0, Json(json!({ "error": self.1.to_string() }))).into_response()
    }
}
