use std::{env, sync::Arc};

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use cat_monitor_rust_backend::{auth::Claims, build_app_from_config, config::Config};
use jsonwebtoken::{encode, EncodingKey, Header};
use serde_json::Value;
use tower::ServiceExt;

#[tokio::test]
async fn tempo_state_uses_migrated_cache_data() {
    let (status, body) = request_rust("/api/tempo/state").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("success").and_then(Value::as_bool), Some(true));
    assert_eq!(body.get("season").and_then(Value::as_str), Some("2025-2026"));
    assert!(body.get("stock_red_remaining").and_then(Value::as_i64).is_some());
    assert!(body.get("stock_white_remaining").and_then(Value::as_i64).is_some());
}

#[tokio::test]
async fn tempo_predictions_return_forecast_entries() {
    let (status, body) = request_rust("/api/tempo/predictions").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("success").and_then(Value::as_bool), Some(true));

    let predictions = body
        .get("predictions")
        .and_then(Value::as_array)
        .expect("predictions should be an array");
    assert_eq!(predictions.len(), 7);
    assert_eq!(
        predictions.first().and_then(|day| day.get("date")).and_then(Value::as_str),
        Some("2026-03-13")
    );
    assert!(predictions.iter().all(|day| day.get("predicted_color").is_some()));
}

#[tokio::test]
async fn tempo_history_reads_cached_season_file() {
    let (status, body) = request_rust("/api/tempo/history?season=2025-2026").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("success").and_then(Value::as_bool), Some(true));

    let history = body
        .get("history")
        .and_then(Value::as_array)
        .expect("history should be an array");
    assert!(!history.is_empty());
    assert!(history.iter().any(|day| {
        day.get("date").and_then(Value::as_str) == Some("2026-01-29")
            && day.get("color").and_then(Value::as_str) == Some("RED")
    }));
}

#[tokio::test]
async fn tempo_calibration_reads_migrated_calibration_file() {
    let (status, body) = request_rust("/api/tempo/calibration").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("success").and_then(Value::as_bool), Some(true));
    assert_eq!(body.get("calibrated").and_then(Value::as_bool), Some(true));
    assert_eq!(
        body.pointer("/params/calibration_date").and_then(Value::as_str),
        Some("2026-03-02")
    );
}

async fn request_rust(path: &str) -> (StatusCode, Value) {
    let config = test_config();
    let token = rust_test_token(&config.jwt_secret);
    let app = build_app_from_config(Arc::new(config)).expect("failed to build test app");

    let request = Request::builder()
        .method("GET")
        .uri(path)
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .expect("request should build");

    let response = app.oneshot(request).await.expect("request should succeed");
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should be readable");
    let json = serde_json::from_slice::<Value>(&body).expect("response should be valid json");
    (status, json)
}

fn test_config() -> Config {
    let source_root = workspace_root();
    Config {
        host: "127.0.0.1".to_string(),
        port: 0,
        jwt_secret: env::var("JWT_SECRET").unwrap_or_else(|_| "super-secret-cat-key-change-me".to_string()),
        disable_bluetooth: true,
        users_path: source_root.join("users.json"),
        meross_devices_path: source_root.join("meross-devices.json"),
        devices_path: source_root.join("devices.json"),
        device_cache_path: source_root.join("device-cache.json"),
        broadlink_codes_path: source_root.join("broadlink-codes.json"),
        hue_lamps_path: source_root.join("hue-lamps.json"),
        hue_blacklist_path: source_root.join("hue-lamps-blacklist.json"),
        source_root,
    }
}

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("backend has parent")
        .to_path_buf()
}

fn rust_test_token(secret: &str) -> String {
    let claims = Claims {
        user_id: "1".to_string(),
        username: "tempo-regression".to_string(),
        role: "admin".to_string(),
        exp: 4_102_444_800,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .expect("test token should encode")
}
