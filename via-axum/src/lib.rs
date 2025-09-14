use std::sync::Arc;

use axum::Router;
use discord_bot::InteractionHandler;
use ed25519_compact::PublicKey;
use secrecy::SecretString;
use snafu::{ResultExt, Snafu};

mod routes;

#[derive(Clone)]
struct AppState {
    discord_application_public_key: PublicKey,
    discord_interaction_handler: InteractionHandler,
    discord_client: Arc<discord_bot::Client>,
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
    let (discord_client, discord_interaction_handler) = discord_bot::init(discord_token.clone())
        .await
        .context(DiscordBotInitSnafu)?;

    let router = routes::create_router();

    let discord_client = Arc::new(discord_client);

    let app_state = AppState {
        discord_application_public_key,
        discord_client,
        discord_interaction_handler,
    };
    let router = router.with_state(app_state);

    Ok(router)
}
