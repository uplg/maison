pub mod auth;
pub mod broadlink;
pub mod config;
pub mod error;
pub mod hue;
pub mod meross;
pub mod routes;
pub mod tempo;
pub mod tuya;

use std::sync::Arc;

use axum::Router;
use broadlink::BroadlinkManager;
use config::Config;
use error::AppError;
use hue::HueManager;
use routes::auth::{load_users, SharedUsers};
use meross::MerossManager;
use tempo::TempoService;
use tuya::TuyaManager;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

#[derive(Clone)]
pub struct AppState {
    pub(crate) config: Arc<Config>,
    pub(crate) users: SharedUsers,
    pub(crate) broadlink: BroadlinkManager,
    pub(crate) hue: HueManager,
    pub(crate) meross: MerossManager,
    pub(crate) tempo: TempoService,
    pub(crate) tuya: TuyaManager,
}

pub fn app_from_env() -> Result<Router, AppError> {
    let config = Arc::new(Config::from_env());
    build_app_from_config(config)
}

pub fn app_parts_from_env() -> Result<(Router, AppState), AppError> {
    let config = Arc::new(Config::from_env());
    build_app_parts_from_config(config)
}

pub fn build_app_from_config(config: Arc<Config>) -> Result<Router, AppError> {
    let (app, _) = build_app_parts_from_config(config)?;
    Ok(app)
}

pub fn build_app_parts_from_config(config: Arc<Config>) -> Result<(Router, AppState), AppError> {
    let users = Arc::new(load_users(&config));
    let broadlink = BroadlinkManager::new(&config.broadlink_codes_path)?;
    let hue = HueManager::new(config.as_ref())?;
    let meross = MerossManager::new(&config.meross_devices_path)?;
    let tempo = TempoService::new(config.source_root.clone())?;
    let tuya = TuyaManager::new(&config.devices_path, &config.device_cache_path)?;

    let state = AppState {
        config,
        users,
        broadlink,
        hue,
        meross,
        tempo,
        tuya,
    };

    let startup_tuya = state.tuya.clone();
    tokio::spawn(async move {
        let device_ids = startup_tuya
            .list_devices()
            .await
            .into_iter()
            .map(|device| device.id)
            .collect::<Vec<_>>();
        for device_id in device_ids {
            let _ = startup_tuya.connect_device(&device_id).await;
        }
    });

    let app = build_app(state.clone());
    Ok((app, state))
}

pub fn build_app(state: AppState) -> Router {
    let api_router = Router::<AppState>::new()
        .merge(routes::root::router())
        .nest("/auth", routes::auth::router())
        .nest("/broadlink", routes::broadlink::router())
        .nest("/devices", routes::devices::router())
        .nest("/hue-lamps", routes::hue::router())
        .nest("/meross", routes::meross::router())
        .nest("/tempo", routes::tempo::router());

    Router::<AppState>::new()
        .merge(routes::root::router())
        .nest("/api", api_router)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

impl AppState {
    pub async fn shutdown(&self) {
        self.hue.shutdown().await;
    }
}
