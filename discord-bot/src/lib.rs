use rspotify::{ClientCredsSpotify, Credentials};
use secrecy::{ExposeSecret, SecretString};
use snafu::{Report, ResultExt, Snafu};
use std::{sync::Arc, time::Duration};
use tokio::{sync::oneshot, time::timeout};
pub use twilight_http::Client;
pub use twilight_model::{
    application::interaction::Interaction, http::interaction::InteractionResponse,
};
use twilight_model::{
    application::interaction::InteractionType,
    channel::message::MessageFlags,
    http::interaction::InteractionResponseType,
    id::{Id, marker::ApplicationMarker},
};
use twilight_util::builder::InteractionResponseDataBuilder;

mod case_insensitive;
mod command;

#[derive(Debug, Clone)]
pub struct State {
    pub discord_client: Arc<Client>,
    pub discord_application_id: Id<ApplicationMarker>,

    pub spotify_client: Arc<ClientCredsSpotify>,
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

#[derive(Debug)]
pub struct InitArgs {
    pub discord_token: SecretString,

    pub spotify_client_id: String,
    pub spotify_client_secret: SecretString,
}

#[tracing::instrument]
pub async fn init(
    InitArgs {
        discord_token,
        spotify_client_id,
        spotify_client_secret,
    }: InitArgs,
) -> Result<(InteractionHandler, State), InitError> {
    let discord_client = Client::new(discord_token.expose_secret().into());

    let current_application = discord_client
        .current_user_application()
        .await
        .context(GetCurrentApplicationSnafu)?
        .model()
        .await
        .context(DeserializeCurrentApplicationSnafu)?;

    let discord_application_id = current_application.id;

    let discord_interaction_client = discord_client.interaction(discord_application_id);

    let all_commands = command::all();

    let discord_commands = Vec::from_iter(
        all_commands
            .iter()
            .map(|(command, _handler)| (*command).to_owned()),
    );

    let _returned_commands = discord_interaction_client
        .set_global_commands(&discord_commands)
        .await
        .context(SetInteractionCommandsSnafu)?
        .models()
        .await
        .context(DeserializeInteractionCommandsSnafu)?;

    let command_router = command::CommandRouter::from_iter(all_commands);

    let interaction_handler = InteractionHandler { command_router };

    let spotify_credentials =
        Credentials::new(&spotify_client_id, spotify_client_secret.expose_secret());
    let spotify_client = ClientCredsSpotify::new(spotify_credentials);

    let discord_client = Arc::new(discord_client);
    let spotify_client = Arc::new(spotify_client);

    let state = State {
        discord_client,
        discord_application_id,
        spotify_client,
    };

    Ok((interaction_handler, state))
}

#[derive(Clone)]
pub struct InteractionHandler {
    command_router: command::CommandRouter,
}

#[derive(Debug, Clone, Snafu)]
pub enum InteractionHandleError {
    #[snafu(display("error handling command"))]
    CommandHandleError { source: command::HandlingError },
}

impl InteractionHandler {
    #[tracing::instrument(skip(self))]
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
                let interaction_token = interaction.token.clone();

                let (tx, rx) = oneshot::channel();

                let command_router = self.command_router.clone();
                let discord_client = state.discord_client.clone();
                let discord_application_id = state.discord_application_id;

                let response_task = tokio::spawn(async move {
                    let ret = command_router.handle(state, interaction).await;
                    tx.send(ret).unwrap();
                });

                match timeout(Duration::from_millis(500), response_task).await {
                    Ok(in_time) => {
                        in_time.unwrap();
                        rx.await.unwrap().context(CommandHandleSnafu)
                    }
                    Err(_) => {
                        tokio::spawn(async move {
                            let response_res = rx.await.unwrap();

                            match response_res {
                                Ok(response) => discord_client
                                    .interaction(discord_application_id)
                                    .update_response(&interaction_token)
                                    .content(
                                        response.data.as_ref().expect("TODO").content.as_deref(),
                                    )
                                    .embeds(response.data.as_ref().expect("TODO").embeds.as_deref())
                                    .await
                                    .unwrap(),
                                Err(handling_error) => discord_client
                                    .interaction(discord_application_id)
                                    .update_response(&interaction_token)
                                    .content(Some(&Report::from_error(handling_error).to_string()))
                                    .await
                                    .unwrap(),
                            }
                        });

                        let deferred = InteractionResponse {
                            kind: InteractionResponseType::DeferredChannelMessageWithSource,
                            data: Some(
                                InteractionResponseDataBuilder::new()
                                    .flags(MessageFlags::EPHEMERAL)
                                    .build(),
                            ),
                        };
                        Ok(deferred)
                    }
                }
            }
            InteractionType::MessageComponent => todo!(),
            InteractionType::ApplicationCommandAutocomplete => todo!(),
            InteractionType::ModalSubmit => todo!(),
            _ => todo!(),
        }
    }
}
