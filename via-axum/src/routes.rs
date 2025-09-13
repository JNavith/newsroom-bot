use axum::Router;

use crate::AppState;

mod discord;

pub fn create_router() -> Router<AppState> {
    Router::new().nest("/discord", discord::create_router())
}
