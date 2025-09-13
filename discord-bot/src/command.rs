use futures::future::BoxFuture;
use rart::{AdaptiveRadixTree, ArrayKey};
use secrecy::SecretString;
use snafu::{OptionExt, Snafu};
use twilight_model::application::{
    command::Command, interaction::application_command::CommandData,
};

mod new_release;

#[derive(Debug, Clone)]
pub struct State {
    discord_token: SecretString,
}

type Return = ();
type BoxedHandler = Box<dyn Fn(State, CommandData) -> BoxFuture<'static, Return>>;

fn box_handler<Handler, Fut>(handler: Handler) -> BoxedHandler
where
    Fut: Future<Output = Return> + Send + 'static,
    Handler: Fn(State, CommandData) -> Fut + 'static,
{
    Box::new(move |state, command_data| Box::pin(handler(state, command_data)))
}

pub fn all() -> Vec<(&'static Command, BoxedHandler)> {
    vec![(&new_release::COMMAND, box_handler(new_release::handle))]
}

#[derive(Default)]
pub struct CommandRouter {
    map: AdaptiveRadixTree<ArrayKey<32>, BoxedHandler>,
}

#[derive(Debug, Snafu)]
pub enum HandlingError {
    #[snafu(display("asked to handle a non-existant command {name:?}"))]
    CommandDoesntExist { name: String },
}

impl CommandRouter {
    fn add<Fut, Handler>(&mut self, name: String, handler: Handler)
    where
        Fut: Future<Output = Return> + Send + 'static,
        Handler: Fn(State, CommandData) -> Fut + 'static,
    {
        self.map.insert(name, box_handler(handler));
    }

    pub async fn handle(
        &self,
        args: State,
        command_data: CommandData,
    ) -> Result<Return, HandlingError> {
        let command_name = &command_data.name;

        let handler = self
            .map
            .get(command_name)
            .with_context(|| CommandDoesntExistSnafu {
                name: command_name.to_owned(),
            })?;

        Ok(handler(args, command_data).await)
    }
}

impl<'a> FromIterator<(&'a CommandData, BoxedHandler)> for CommandRouter {
    fn from_iter<T: IntoIterator<Item = (&'a CommandData, BoxedHandler)>>(iter: T) -> Self {
        let mut router = CommandRouter::default();

        for (command, handler) in iter {
            let name = &command.name;
            router.add(name.to_owned(), handler);
        }

        router
    }
}
