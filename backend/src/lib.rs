pub mod auth;
pub mod broadlink;
pub mod config;
pub mod error;
#[cfg(feature = "bluetooth")]
pub mod hue;
#[cfg(not(feature = "bluetooth"))]
#[path = "hue_stub.rs"]
pub mod hue;
pub mod meross;
pub mod mitsubishi_ir;
pub mod routes;
pub mod tempo;
pub mod tuya;
pub mod zigbee;

pub use tempo::TempoService;

use std::sync::Arc;

use axum::Router;
use auth::AuthRateLimiter;
use broadlink::BroadlinkManager;
use config::Config;
use error::AppError;
use hue::HueManager;
use tower_http::services::{ServeDir, ServeFile};
use routes::auth::{load_users, SharedUsers};
use meross::MerossManager;
use tuya::TuyaManager;
use tower_http::trace::TraceLayer;
use zigbee::ZigbeeManager;

#[derive(Clone)]
pub struct AppState {
    pub(crate) config: Arc<Config>,
    pub(crate) users: SharedUsers,
    pub(crate) auth_rate_limiter: AuthRateLimiter,
    pub(crate) broadlink: BroadlinkManager,
    pub(crate) hue: HueManager,
    pub(crate) meross: MerossManager,
    pub(crate) tempo: TempoService,
    pub(crate) tuya: TuyaManager,
    pub(crate) zigbee: ZigbeeManager,
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
    let users = Arc::new(load_users(&config)?);
    let auth_rate_limiter = AuthRateLimiter::default();
    let broadlink = BroadlinkManager::new(&config.broadlink_codes_path)?;
    let hue = HueManager::new(config.as_ref())?;
    let meross = MerossManager::new(&config.meross_devices_path)?;
    let tempo = TempoService::new(config.source_root.clone())?;
    let tuya = TuyaManager::new(&config.devices_path, &config.device_cache_path)?;
    let zigbee = ZigbeeManager::new(config.as_ref())?;

    let state = AppState {
        config,
        users,
        auth_rate_limiter,
        broadlink,
        hue,
        meross,
        tempo,
        tuya,
        zigbee,
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
        .merge(routes::root::api_router())
        .nest("/auth", routes::auth::router())
        .nest("/broadlink", routes::broadlink::router())
        .nest("/devices", routes::devices::router())
        .nest("/hue-lamps", routes::hue::router())
        .nest("/meross", routes::meross::router())
        .nest("/tempo", routes::tempo::router())
        .nest("/zigbee", routes::zigbee::router());

    let app = Router::<AppState>::new()
        .merge(routes::root::health_router())
        .nest("/api", api_router);

    let app = if state.config.frontend_dist_dir.join("index.html").is_file() {
        app.fallback_service(
            ServeDir::new(state.config.frontend_dist_dir.clone())
                .not_found_service(ServeFile::new(state.config.frontend_dist_dir.join("index.html"))),
        )
    } else {
        app.merge(routes::root::router())
    };

    app.layer(TraceLayer::new_for_http()).with_state(state)
}

impl AppState {
    pub fn validate_runtime_security(&self) -> Result<(), AppError> {
        if self.config.jwt_secret == "super-secret-cat-key-change-me" {
            return Err(AppError::http(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Refusing to start with the default JWT secret. Set JWT_SECRET in .env.",
            ));
        }

        Ok(())
    }

    pub async fn shutdown(&self) {
        self.hue.shutdown().await;
        self.zigbee.shutdown().await;
    }
}
