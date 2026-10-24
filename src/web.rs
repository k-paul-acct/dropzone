use axum::{Router, http::header, response::IntoResponse, routing::get};

pub fn assets() -> axum::Router {
    Router::new()
        .route("/style.css", get(stylesheet))
        .route("/favicon.svg", get(favicon))
}

async fn stylesheet() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("style.css"),
    )
}

async fn favicon() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "image/svg+xml")], include_str!("favicon.svg"))
}
