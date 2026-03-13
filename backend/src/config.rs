use std::{env, path::PathBuf};

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub jwt_secret: String,
    pub disable_bluetooth: bool,
    pub source_root: PathBuf,
    pub users_path: PathBuf,
    pub meross_devices_path: PathBuf,
    pub devices_path: PathBuf,
    pub device_cache_path: PathBuf,
    pub broadlink_codes_path: PathBuf,
    pub hue_lamps_path: PathBuf,
    pub hue_blacklist_path: PathBuf,
}

impl Config {
    pub fn from_env() -> Self {
        let source_root = env::var("CAT_MONITOR_SOURCE_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| default_source_root());

        let users_path = env::var("USERS_JSON_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| source_root.join("users.json"));

        let meross_devices_path = env::var("MEROSS_DEVICES_JSON_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| source_root.join("meross-devices.json"));

        let devices_path = env::var("DEVICES_JSON_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| source_root.join("devices.json"));

        let device_cache_path = env::var("DEVICE_CACHE_JSON_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| source_root.join("device-cache.json"));

        let broadlink_codes_path = env::var("BROADLINK_CODES_JSON_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| source_root.join("broadlink-codes.json"));

        let hue_lamps_path = env::var("HUE_LAMPS_JSON_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| source_root.join("hue-lamps.json"));

        let hue_blacklist_path = env::var("HUE_BLACKLIST_JSON_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| source_root.join("hue-lamps-blacklist.json"));

        let disable_bluetooth = env::var("DISABLE_BLUETOOTH")
            .map(|value| {
                matches!(
                    value.to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false);

        let port = env::var("PORT")
            .ok()
            .or_else(|| env::var("API_PORT").ok())
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(3033);

        let host = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let jwt_secret =
            env::var("JWT_SECRET").unwrap_or_else(|_| "super-secret-cat-key-change-me".to_string());

        Self {
            host,
            port,
            jwt_secret,
            disable_bluetooth,
            source_root,
            users_path,
            meross_devices_path,
            devices_path,
            device_cache_path,
            broadlink_codes_path,
            hue_lamps_path,
            hue_blacklist_path,
        }
    }
}

fn default_source_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("backend has parent")
        .to_path_buf()
}
