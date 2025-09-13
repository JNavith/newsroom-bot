use axum::Router;
use secrecy::SecretString;
use snafu::{ResultExt, Snafu};

mod routes;

#[derive(Debug, Clone)]
struct AppState {
    // TODO
}

#[derive(Debug, Snafu)]
pub enum InitError {
    #[snafu(display("couldn't initialize the discord bot"))]
    DiscordBotInitError { source: discord_bot::InitError },
}

#[tracing::instrument]
pub async fn init(discord_token: SecretString) -> Result<Router<()>, InitError> {
    let something = discord_bot::init(discord_token)
        .await
        .context(DiscordBotInitSnafu)?; // TODO

    let router = routes::create_router();

    let app_state = AppState {}; // TODO
    let router = router.with_state(app_state);

    Ok(router)
}
