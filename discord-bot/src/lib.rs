use std::error::Error;

use secrecy::{ExposeSecret, SecretString};
use snafu::{ResultExt, Snafu};

mod command;

#[derive(Debug, Snafu)]
pub enum InitError {
    #[snafu(display("couldn't get current Discord application"))]
    CouldntGetCurrentDiscordApplicationError { source: twilight_http::Error },
}

#[tracing::instrument]
pub async fn init(discord_token: SecretString) -> Result<(), InitError> {
    let discord_client = twilight_http::Client::new(discord_token.expose_secret().into());

    let current_application = discord_client
        .current_user_application()
        .await
        .context(CouldntGetCurrentDiscordApplicationSnafu)?;

    todo!();
}
