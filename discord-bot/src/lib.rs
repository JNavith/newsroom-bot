use secrecy::{ExposeSecret, SecretString};
use snafu::{ResultExt, Snafu};

mod command;

#[derive(Debug, Snafu)]
pub enum InitError {
    #[snafu(display("couldn't get current Discord application"))]
    GetCurrentApplicationError { source: twilight_http::Error },
    #[snafu(display("couldn't deserialize current Discord application"))]
    DeserializeCurrentApplicationError {
        source: twilight_http::response::DeserializeBodyError,
    },

    #[snafu(display("couldn't set the Discord interaction commands"))]
    SetInteractionCommandsError { source: twilight_http::Error },
    #[snafu(display("couldn't deserialize the returned Discord interaction commands"))]
    DeserializeInteractionCommandsError {
        source: twilight_http::response::DeserializeBodyError,
    },
}

#[tracing::instrument]
pub async fn init(discord_token: SecretString) -> Result<(), InitError> {
    let discord_client = twilight_http::Client::new(discord_token.expose_secret().into());

    let current_application = discord_client
        .current_user_application()
        .await
        .context(GetCurrentApplicationSnafu)?
        .model()
        .await
        .context(DeserializeCurrentApplicationSnafu)?;

    let application_id = current_application.id;

    let interaction_client = discord_client.interaction(application_id);

    let all_commands = command::all();

    let discord_commands = Vec::from_iter(
        all_commands
            .iter()
            .map(|(command, _handler)| (*command).to_owned()),
    );

    let _returned_commands = interaction_client
        .set_global_commands(&discord_commands)
        .await
        .context(SetInteractionCommandsSnafu)?
        .models()
        .await
        .context(DeserializeInteractionCommandsSnafu)?;

    Ok(())
}
