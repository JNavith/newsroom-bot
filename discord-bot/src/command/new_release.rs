use std::sync::LazyLock;

use itertools::Itertools;
use snafu::{OptionExt, ResultExt, Snafu};
use twilight_model::{
    application::{
        command::{Command, CommandType},
        interaction::application_command::CommandData,
    },
    http::interaction::{InteractionResponse, InteractionResponseType},
};
use twilight_util::builder::{
    InteractionResponseDataBuilder,
    command::{CommandBuilder, StringBuilder},
};

use crate::command::State;

const NAME: &str = "new-release";
const DESCRIPTION: &str = "Post a new music release in this channel";

const URL_NAME: &str = "url";
const URL_DESCRIPTION: &str = "The URL to the release on Spotify (only service supported so far)";

pub static COMMAND: LazyLock<Command> = LazyLock::new(|| {
    CommandBuilder::new(NAME, DESCRIPTION, CommandType::ChatInput)
        .option(StringBuilder::new(URL_NAME, URL_DESCRIPTION).required(true))
        .validate()
        .expect("command wasn't correct")
        .build()
});

#[derive(Debug, Snafu)]
enum HandleError {
    #[snafu(display("the command was run outside of a Discord server"))]
    NotUsedInGuild,
    #[snafu(display("couldn't get the roles in this Discord server"))]
    GetRolesError { source: twilight_http::Error },
    #[snafu(display("couldn't deserialize the returned roles in this Discord server"))]
    DeserializeRolesError {
        source: twilight_http::response::DeserializeBodyError,
    },
}

impl From<HandleError> for InteractionResponse {
    fn from(error: HandleError) -> Self {
        use HandleError as E;

        match error {
            E::NotUsedInGuild => {
                // TODO: consider using `Report::from_error(error).to_string()` instead?
                let interaction_response_data = InteractionResponseDataBuilder::new()
                    .content("sorry but you have to use this inside a server")
                    .build();

                InteractionResponse {
                    kind: InteractionResponseType::ChannelMessageWithSource,
                    data: Some(interaction_response_data),
                }
            }
            // Not showing the underlying error to users
            E::GetRolesError { source: _ } => {
                let interaction_response_data = InteractionResponseDataBuilder::new()
                    .content("couldn't get roles in this discord server, sorry")
                    .build();

                InteractionResponse {
                    kind: InteractionResponseType::ChannelMessageWithSource,
                    data: Some(interaction_response_data),
                }
            }
            // Not showing the underlying error to users
            E::DeserializeRolesError { source: _ } => {
                let interaction_response_data = InteractionResponseDataBuilder::new()
                    .content("got roles from discord but not in a format I understand, sorry")
                    .build();

                InteractionResponse {
                    kind: InteractionResponseType::ChannelMessageWithSource,
                    data: Some(interaction_response_data),
                }
            }
        }
    }
}

#[tracing::instrument(ret)]
async fn handle_impl(state: State, data: CommandData) -> Result<InteractionResponse, HandleError> {
    let guild_id = data.guild_id.context(NotUsedInGuildSnafu)?;

    let discord_client = state.discord_client;

    let roles = discord_client
        .roles(guild_id)
        .await
        .context(GetRolesSnafu)?
        .models()
        .await
        .context(DeserializeRolesSnafu)?;

    tracing::error!(?roles, "todo: do something with them");

    let interaction_response_data = InteractionResponseDataBuilder::new()
        .content(format!(
            "hey this is a work in progress, here are roles in this server: {}",
            roles.into_iter().map(|role| role.name).join("\n")
        ))
        .build();

    Ok(InteractionResponse {
        kind: InteractionResponseType::ChannelMessageWithSource,
        data: Some(interaction_response_data),
    })
}

#[tracing::instrument]
pub async fn handle(state: State, data: CommandData) -> InteractionResponse {
    match handle_impl(state, data).await {
        Ok(interaction_response) => interaction_response,
        Err(error) => error.into(),
    }
}
