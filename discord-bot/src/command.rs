use std::sync::Arc;

use crate::State;
use futures::future::BoxFuture;
use rart::{ArrayKey, VersionedAdaptiveRadixTree};
use snafu::{OptionExt, Snafu};
use twilight_model::{
    application::{command::Command, interaction::application_command::CommandData},
    http::interaction::InteractionResponse,
};

mod new_release;

type Return = InteractionResponse;
type ArcedHandler = Arc<dyn Fn(State, CommandData) -> BoxFuture<'static, Return> + Send + Sync>;

fn arc_handler<Handler, Fut>(handler: Handler) -> ArcedHandler
where
    Fut: Future<Output = Return> + Send + 'static,
    Handler: Send + Sync + Fn(State, CommandData) -> Fut + 'static,
{
    Arc::new(move |state, command_data| Box::pin(handler(state, command_data)))
}

pub fn all() -> Vec<(&'static Command, ArcedHandler)> {
    vec![(&new_release::COMMAND, arc_handler(new_release::handle))]
}

#[derive(Default, Clone)]
pub struct CommandRouter {
    map: VersionedAdaptiveRadixTree<ArrayKey<32>, ArcedHandler>,
}

#[derive(Debug, Clone, Snafu)]
pub enum HandlingError {
    #[snafu(display("asked to handle a non-existant command {name:?}"))]
    CommandDoesntExist { name: String },
}

impl CommandRouter {
    fn add<Fut, Handler>(&mut self, name: String, handler: Handler)
    where
        Fut: Future<Output = Return> + Send + 'static,
        Handler: Send + Sync + Fn(State, CommandData) -> Fut + 'static,
    {
        self.add_already_arced(name, arc_handler(handler));
    }

    fn add_already_arced(&mut self, name: String, handler: ArcedHandler) {
        self.map.insert(name, handler);
    }

    pub async fn handle(
        &self,
        state: State,
        command_data: CommandData,
    ) -> Result<Return, HandlingError> {
        let command_name = &command_data.name;

        let handler = self
            .map
            .get(command_name)
            .with_context(|| CommandDoesntExistSnafu {
                name: command_name.to_owned(),
            })?;

        Ok(handler(state, command_data).await)
    }
}

impl<'a> FromIterator<(&'a Command, ArcedHandler)> for CommandRouter {
    fn from_iter<T: IntoIterator<Item = (&'a Command, ArcedHandler)>>(iter: T) -> Self {
        let mut router = CommandRouter::default();

        for (command, handler) in iter {
            let name = &command.name;
            router.add_already_arced(name.to_owned(), handler);
        }

        router
    }
}
