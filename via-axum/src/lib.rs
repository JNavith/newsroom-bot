use axum::Router;
use ed25519_compact::PublicKey;
use secrecy::SecretString;
use snafu::{ResultExt, Snafu};

mod routes;

#[derive(Debug, Clone)]
struct AppState {
    discord_application_public_key: PublicKey,
}

#[derive(Debug, Snafu)]
pub enum InitError {
    #[snafu(display("couldn't initialize the discord bot"))]
    DiscordBotInitError { source: discord_bot::InitError },
}

#[tracing::instrument]
pub async fn init(
    discord_token: SecretString,
    discord_application_public_key: PublicKey,
) -> Result<Router<()>, InitError> {
    let something = discord_bot::init(discord_token)
        .await
        .context(DiscordBotInitSnafu)?; // TODO

    let router = routes::create_router();

    let app_state = AppState {
        discord_application_public_key,
    };
    let router = router.with_state(app_state);

    Ok(router)
}
