use crate::AppState;
use axum::Router;

mod interactions;

pub fn create_router() -> Router<AppState> {
    Router::new().nest("/interactions", interactions::create_router())
}
