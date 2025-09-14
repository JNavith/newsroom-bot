use std::sync::LazyLock;

use twilight_model::{
    application::{
        command::{Command, CommandType},
        interaction::application_command::CommandData,
    },
    http::interaction::{InteractionResponse, InteractionResponseType},
};
use twilight_util::builder::{InteractionResponseDataBuilder, command::CommandBuilder};

use crate::command::State;

const NAME: &str = "new-release";
const DESCRIPTION: &str = "Post a new music release in this channel";

pub static COMMAND: LazyLock<Command> = LazyLock::new(|| {
    CommandBuilder::new(NAME, DESCRIPTION, CommandType::ChatInput)
        .validate()
        .expect("command wasn't correct")
        .build()
});

#[tracing::instrument]
pub async fn handle(state: State, data: CommandData) -> InteractionResponse {
    let interaction_response_data = InteractionResponseDataBuilder::new()
        .content("hey this is a work in progress")
        .build();

    InteractionResponse {
        kind: InteractionResponseType::ChannelMessageWithSource,
        data: Some(interaction_response_data),
    }
}
