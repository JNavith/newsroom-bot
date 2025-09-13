use std::sync::LazyLock;

use twilight_model::application::{
    command::{Command, CommandType},
    interaction::application_command::CommandData,
};
use twilight_util::builder::command::CommandBuilder;

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
pub async fn handle(state: State, data: CommandData) {
    todo!();
}
