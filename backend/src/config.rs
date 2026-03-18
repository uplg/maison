use std::{env, path::PathBuf};

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub jwt_secret: String,
    pub auth_cookie_name: String,
    pub auth_cookie_secure: bool,
    pub auth_rate_limit_attempts: u32,
    pub auth_rate_limit_window_seconds: i64,
    pub disable_bluetooth: bool,
    pub source_root: PathBuf,
    pub users_path: PathBuf,
    pub meross_devices_path: PathBuf,
    pub devices_path: PathBuf,
    pub device_cache_path: PathBuf,
    pub broadlink_codes_path: PathBuf,
    pub hue_lamps_path: PathBuf,
    pub hue_blacklist_path: PathBuf,
    pub zigbee_lamps_path: PathBuf,
    pub zigbee_lamps_blacklist_path: PathBuf,
    pub mqtt_host: String,
    pub mqtt_port: u16,
    pub mqtt_username: Option<String>,
    pub mqtt_password: Option<String>,
    pub mqtt_client_id: String,
    pub z2m_base_topic: String,
    pub zigbee_permit_join_seconds: u16,
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

        let zigbee_lamps_path = env::var("ZIGBEE_LAMPS_JSON_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| source_root.join("zigbee-lamps.json"));

        let zigbee_lamps_blacklist_path = env::var("ZIGBEE_LAMPS_BLACKLIST_JSON_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| source_root.join("zigbee-lamps-blacklist.json"));

        let disable_bluetooth = env::var("DISABLE_BLUETOOTH")
            .map(|value| {
                matches!(
                    value.to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false);

        let auth_cookie_name =
            env::var("AUTH_COOKIE_NAME").unwrap_or_else(|_| "maison_session".to_string());

        let auth_cookie_secure = env::var("AUTH_COOKIE_SECURE")
            .map(|value| {
                matches!(
                    value.to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(true);

        let auth_rate_limit_attempts = env::var("AUTH_RATE_LIMIT_ATTEMPTS")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(10);

        let auth_rate_limit_window_seconds = env::var("AUTH_RATE_LIMIT_WINDOW_SECONDS")
            .ok()
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(300);

        let port = env::var("PORT")
            .ok()
            .or_else(|| env::var("API_PORT").ok())
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(3033);

        let host = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let jwt_secret =
            env::var("JWT_SECRET").unwrap_or_else(|_| "super-secret-cat-key-change-me".to_string());
        let mqtt_host = env::var("MQTT_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let mqtt_port = env::var("MQTT_PORT")
            .ok()
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(1883);
        let mqtt_username = env::var("MQTT_USERNAME").ok().and_then(|value| {
            if value.is_empty() {
                None
            } else {
                Some(value)
            }
        });
        let mqtt_password = env::var("MQTT_PASSWORD").ok().and_then(|value| {
            if value.is_empty() {
                None
            } else {
                Some(value)
            }
        });
        let mqtt_client_id =
            env::var("MQTT_CLIENT_ID").unwrap_or_else(|_| "cat-monitor-backend".to_string());
        let z2m_base_topic =
            env::var("Z2M_BASE_TOPIC").unwrap_or_else(|_| "zigbee2mqtt".to_string());
        let zigbee_permit_join_seconds = env::var("ZIGBEE_PERMIT_JOIN_SECONDS")
            .ok()
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(120);

        Self {
            host,
            port,
            jwt_secret,
            auth_cookie_name,
            auth_cookie_secure,
            auth_rate_limit_attempts,
            auth_rate_limit_window_seconds,
            disable_bluetooth,
            source_root,
            users_path,
            meross_devices_path,
            devices_path,
            device_cache_path,
            broadlink_codes_path,
            hue_lamps_path,
            hue_blacklist_path,
            zigbee_lamps_path,
            zigbee_lamps_blacklist_path,
            mqtt_host,
            mqtt_port,
            mqtt_username,
            mqtt_password,
            mqtt_client_id,
            z2m_base_topic,
            zigbee_permit_join_seconds,
        }
    }
}

fn default_source_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("backend has parent")
        .to_path_buf()
}
