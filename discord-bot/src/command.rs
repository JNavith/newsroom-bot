use std::sync::Arc;

use crate::State;
use futures::future::BoxFuture;
use rart::{ArrayKey, VersionedAdaptiveRadixTree};
use snafu::{OptionExt, Snafu};
use twilight_model::{
    application::{
        command::Command,
        interaction::{Interaction, InteractionData},
    },
    http::interaction::InteractionResponse,
};

mod new_release;

type Return = InteractionResponse;
type ArcedHandler = Arc<dyn Fn(State, Interaction) -> BoxFuture<'static, Return> + Send + Sync>;

fn arc_handler<Handler, Fut>(handler: Handler) -> ArcedHandler
where
    Fut: Future<Output = Return> + Send + 'static,
    Handler: Send + Sync + Fn(State, Interaction) -> Fut + 'static,
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
    #[snafu(display("missing expected interaction data"))]
    MisssingInteractionData,
    #[snafu(display("missing expected command data"))]
    MissingExpectedCommandData,

    #[snafu(display("asked to handle a non-existant command {name:?}"))]
    CommandDoesntExist { name: String },
}

impl CommandRouter {
    fn add<Fut, Handler>(&mut self, name: String, handler: Handler)
    where
        Fut: Future<Output = Return> + Send + 'static,
        Handler: Send + Sync + Fn(State, Interaction) -> Fut + 'static,
    {
        self.add_already_arced(name, arc_handler(handler));
    }

    fn add_already_arced(&mut self, name: String, handler: ArcedHandler) {
        self.map.insert(name, handler);
    }

    pub async fn handle(
        &self,
        state: State,
        interaction: Interaction,
    ) -> Result<Return, HandlingError> {
        let InteractionData::ApplicationCommand(command_data) = interaction
            .data
            .as_ref()
            .context(MisssingInteractionDataSnafu)?
        else {
            return Err(HandlingError::MissingExpectedCommandData);
        };

        let command_name = &command_data.name;

        let handler = self
            .map
            .get(command_name)
            .with_context(|| CommandDoesntExistSnafu {
                name: command_name.to_owned(),
            })?;

        Ok(handler(state, interaction).await)
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
