use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};


use crate::AppState;

/// 认证中间件
/// 检查请求头中的 Authorization: Bearer <token>
pub async fn auth_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // 获取配置的 token
    let expected_token = {
        let state = state.read().await;
        state.config.token.clone()
    };

    // 从请求头获取 token
    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());

    match auth_header {
        Some(header) if header.starts_with("Bearer ") => {
            let token = &header[7..]; // 跳过 "Bearer "
            if token == expected_token {
                Ok(next.run(request).await)
            } else {
                tracing::warn!("Invalid token provided");
                Err(StatusCode::UNAUTHORIZED)
            }
        }
        _ => {
            tracing::warn!("Missing or invalid Authorization header");
            Err(StatusCode::UNAUTHORIZED)
        }
    }
}

/// 健康检查不需要认证
pub async fn health_check() -> &'static str {
    tracing::info!("Health check requested");
    "OK"
}
