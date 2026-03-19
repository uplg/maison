use std::{env, sync::Arc};

use axum::{
    body::{to_bytes, Body},
    http::{Method, Request, StatusCode},
};
use cat_monitor_rust_backend::{auth::Claims, build_app_from_config, config::Config};
use jsonwebtoken::{encode, EncodingKey, Header};
use serde_json::{json, Value};
use tower::ServiceExt;

#[tokio::test]
async fn auth_verify_accepts_valid_token() {
    let rust = request_rust(Method::POST, "/api/auth/verify", None, Some(rust_test_token())).await;

    assert_eq!(rust.0, StatusCode::OK);
    assert_eq!(
        normalize_verify_response(rust.1),
        json!({
            "success": true,
            "user": {
                "id": "1",
                "username": "leonard",
                "role": "admin"
            }
        })
    );
}

#[tokio::test]
async fn auth_logout_returns_success_message() {
    let rust = request_rust(Method::POST, "/api/auth/logout", None, None).await;

    assert_eq!(rust.0, StatusCode::OK);
    assert_eq!(
        rust.1,
        json!({
            "success": true,
            "message": "Logged out successfully"
        })
    );
}

#[tokio::test]
async fn auth_invalid_login_returns_unauthorized() {
    let body = json!({ "username": "nope", "password": "nope" });
    let rust = request_rust(Method::POST, "/api/auth/login", Some(body), None).await;

    assert_eq!(rust.0, StatusCode::UNAUTHORIZED);
    assert_eq!(
        rust.1,
        json!({
            "success": false,
            "error": "Invalid username or password"
        })
    );
}

async fn request_rust(
    method: Method,
    path: &str,
    body: Option<Value>,
    token: Option<String>,
) -> (StatusCode, Value) {
    let config = test_config();
    let app = build_app_from_config(Arc::new(config)).expect("failed to build test app");

    let mut builder = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        builder = builder.header("Authorization", format!("Bearer {token}"));
    }
    if body.is_some() {
        builder = builder.header("Content-Type", "application/json");
    }

    let request_body = body
        .map(|value| Body::from(serde_json::to_vec(&value).expect("body json should encode")))
        .unwrap_or_else(Body::empty);
    let request = builder.body(request_body).expect("request should build");

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
        frontend_dist_dir: source_root.join("frontend").join("dist"),
        auth_cookie_name: "maison_session".to_string(),
        auth_cookie_secure: false,
        auth_rate_limit_attempts: 10,
        auth_rate_limit_window_seconds: 300,
        disable_bluetooth: true,
        users_path: source_root.join("users.json"),
        meross_devices_path: source_root.join("meross-devices.json"),
        devices_path: source_root.join("devices.json"),
        device_cache_path: source_root.join("device-cache.json"),
        broadlink_codes_path: source_root.join("broadlink-codes.json"),
        hue_lamps_path: source_root.join("hue-lamps.json"),
        hue_blacklist_path: source_root.join("hue-lamps-blacklist.json"),
        zigbee_lamps_path: source_root.join("zigbee-lamps.json"),
        zigbee_lamps_blacklist_path: source_root.join("zigbee-lamps-blacklist.json"),
        mqtt_host: "127.0.0.1".to_string(),
        mqtt_port: 1883,
        mqtt_username: None,
        mqtt_password: None,
        mqtt_client_id: "cat-monitor-test".to_string(),
        z2m_base_topic: "zigbee2mqtt".to_string(),
        zigbee_permit_join_seconds: 120,
        source_root,
    }
}

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("backend has parent")
        .to_path_buf()
}

fn rust_test_token() -> String {
    let claims = Claims {
        user_id: "1".to_string(),
        username: "leonard".to_string(),
        role: "admin".to_string(),
        exp: 4_102_444_800,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(
            env::var("JWT_SECRET")
                .unwrap_or_else(|_| "super-secret-cat-key-change-me".to_string())
                .as_bytes(),
        ),
    )
    .expect("test token should encode")
}

fn normalize_verify_response(value: Value) -> Value {
    json!({
        "success": value.get("success").cloned().unwrap_or(Value::Null),
        "user": value.get("user").cloned().unwrap_or(Value::Null),
    })
}
