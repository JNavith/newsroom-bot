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

    discord_bot_state: discord_bot::State,
}

#[derive(Debug)]
pub struct InitArgs {
    pub discord_token: SecretString,
    pub discord_application_public_key: PublicKey,
    pub spotify_client_id: String,
    pub spotify_client_secret: SecretString,
}

#[derive(Debug, Snafu)]
pub enum InitError {
    #[snafu(display("couldn't initialize the discord bot"))]
    DiscordBotInitError { source: discord_bot::InitError },
}

#[tracing::instrument]
pub async fn init(
    InitArgs {
        discord_token,
        discord_application_public_key,
        spotify_client_id,
        spotify_client_secret,
    }: InitArgs,
) -> Result<Router<()>, InitError> {
    let (discord_interaction_handler, discord_bot_state) =
        discord_bot::init(discord_bot::InitArgs {
            discord_token,
            spotify_client_id,
            spotify_client_secret,
        })
        .await
        .context(DiscordBotInitSnafu)?;

    let router = routes::create_router();

    let app_state = AppState {
        discord_application_public_key,
        discord_interaction_handler,
        discord_bot_state,
    };
    let router = router.with_state(app_state);

    Ok(router)
}
