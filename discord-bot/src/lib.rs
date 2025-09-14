use secrecy::{ExposeSecret, SecretString};
use snafu::{ResultExt, Snafu};
pub use twilight_model::{
    application::interaction::Interaction, http::interaction::InteractionResponse,
};
use twilight_model::{
    application::interaction::{InteractionData, InteractionType},
    http::interaction::InteractionResponseType,
};

mod command;

#[derive(Debug, Clone)]
pub struct State {
    pub discord_token: SecretString,
}

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
pub async fn init(discord_token: SecretString) -> Result<InteractionHandler, InitError> {
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

    let command_router = command::CommandRouter::from_iter(all_commands);

    let interaction_handler = InteractionHandler { command_router };

    Ok(interaction_handler)
}

#[derive(Clone)]
pub struct InteractionHandler {
    command_router: command::CommandRouter,
}

#[derive(Debug, Clone, Snafu)]
pub enum InteractionHandleError {
    #[snafu(display("error handling command"))]
    CommandHandleError { source: command::HandlingError },
    #[snafu(display("missing expected command data"))]
    MissingExpectedCommandData,
}

impl InteractionHandler {
    pub async fn handle(
        &self,
        state: State,
        interaction: Interaction,
    ) -> Result<InteractionResponse, InteractionHandleError> {
        match interaction.kind {
            InteractionType::Ping => Ok(InteractionResponse {
                kind: InteractionResponseType::Pong,
                data: None,
            }),
            InteractionType::ApplicationCommand => {
                let Some(InteractionData::ApplicationCommand(command_data)) = interaction.data
                else {
                    return Err(InteractionHandleError::MissingExpectedCommandData);
                };

                let command_data = *command_data;

                let interaction_response = self
                    .command_router
                    .handle(state, command_data)
                    .await
                    .context(CommandHandleSnafu)?;

                Ok(interaction_response)
            }
            InteractionType::MessageComponent => todo!(),
            InteractionType::ApplicationCommandAutocomplete => todo!(),
            InteractionType::ModalSubmit => todo!(),
            _ => todo!(),
        }
    }
}
