use ahash::AHashSet;
use deranged::RangedU8;
use futures::TryStreamExt;
use iref::{
    InvalidIri, Iri, IriRef, IriRefBuf,
    iri::{InvalidIriRef, SegmentBuf},
};
use itertools::Itertools;
use nonempty::NonEmpty as NonEmptyVec;
use rspotify::{
    model::{AlbumId, AlbumType, Id, IdError, PlaylistId, TrackId},
    prelude::BaseClient,
};
use snafu::{OptionExt, Report, ResultExt, Snafu};
use std::{collections::BTreeMap, num::ParseIntError, sync::LazyLock};
use time::{Date, OffsetDateTime, Time};
use twilight_model::{
    application::{
        command::{Command, CommandType},
        interaction::{
            Interaction, InteractionData,
            application_command::{CommandDataOption, CommandOptionValue},
        },
    },
    channel::message::MessageFlags,
    guild::Role,
    http::interaction::{InteractionResponse, InteractionResponseType},
};
use twilight_util::builder::{
    InteractionResponseDataBuilder,
    command::{CommandBuilder, StringBuilder},
    embed::{EmbedBuilder, EmbedFooterBuilder},
};

use crate::{case_insensitive::CaseInsensitiveString, command::State};

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

#[derive(Debug, Clone)]
enum SpotifyResource<'a> {
    Album { id: AlbumId<'a> },
    Track { id: TrackId<'a> },
    Playlist { id: PlaylistId<'a> },
}

#[derive(Debug, Clone, Snafu)]
enum SpotifyResourceFromUrlError {
    #[snafu(display("no resource type in the URL"))]
    MissingResourceType,
    #[snafu(display("no resource ID in the URL"))]
    MissingResourceId,
    #[snafu(display(
        "the resource type in the URL ({kind:?}) is not one that I recognize (e.g. album)"
    ))]
    UnrecognizedResourceType { kind: SegmentBuf },
    #[snafu(display("the resource ID in the URL ({id:?}) is not valid by Spotify's rules"))]
    InvalidResourceId { id: String, source: IdError },
}

fn parse_spotify_resource<'a>(
    url: &'a IriRef,
    base: &'a IriRef,
) -> Result<SpotifyResource<'static>, SpotifyResourceFromUrlError> {
    let resource_path = url.relative_to(base);

    let mut segments = resource_path.path().segments();

    let kind = segments.next().context(MissingResourceTypeSnafu)?;
    let id = segments.next().context(MissingResourceIdSnafu)?;

    let kind = kind.to_owned();
    match kind.as_str() {
        "album" => Ok(SpotifyResource::Album {
            id: AlbumId::from_id(id.as_str())
                .with_context(|_e| InvalidResourceIdSnafu { id: id.to_owned() })?
                .into_static(),
        }),
        "playlist" => Ok(SpotifyResource::Playlist {
            id: PlaylistId::from_id(id.as_str())
                .with_context(|_e| InvalidResourceIdSnafu { id: id.to_owned() })?
                .into_static(),
        }),
        "track" => Ok(SpotifyResource::Track {
            id: TrackId::from_id(id.as_str())
                .with_context(|_e| InvalidResourceIdSnafu { id: id.to_owned() })?
                .into_static(),
        }),
        _other => Err(SpotifyResourceFromUrlError::UnrecognizedResourceType { kind }),
    }
}

type Year = u16;
type Month = RangedU8<1, 12>;
type Day = RangedU8<1, 32>;

type DayResult = Result<Day, deranged::ParseIntError>;
type MonthResult = Result<(Month, Option<DayResult>), deranged::ParseIntError>;
type YearResult = Result<(Year, Option<MonthResult>), ParseIntError>;

fn try_split_once<'a>(string: &'a str, delimiter: &'a str) -> (&'a str, Option<&'a str>) {
    match string.split_once(delimiter) {
        Some((before, after)) => (before, Some(after)),
        None => (string, None),
    }
}

fn parse_date(date: &str) -> YearResult {
    let (year, month_and_date) = try_split_once(date, "-");

    year.parse().map(|year| {
        (
            year,
            month_and_date.map(|month_and_date| {
                let (month, day) = try_split_once(month_and_date, "-");
                month
                    .parse()
                    .map(|month| (month, day.map(|day| day.parse())))
            }),
        )
    })
}

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

    #[snafu(display("the `url` argument wasn't provided"))]
    UrlMissing,

    #[snafu(display(
        "the `url` argument wasn't a string like it's supposed to be, it was actually {actual:?}"
    ))]
    UrlNotString { actual: CommandOptionValue },

    #[snafu(display("the `url` argument couldn't be parsed as a URL"))]
    UrlParseError { source: InvalidIriRef<String> },

    #[snafu(display("the base URL of my Spotify client isn't actually a valid URL?!"))]
    SpotifyBaseNotValidUrl { source: InvalidIri<&'static str> },

    #[snafu(display("the `url` isn't to a supported service (currently just Spotify)"))]
    UrlForUnsupportedService { source: SpotifyResourceFromUrlError },

    #[snafu(display(
        "the `url` is for Spotify, but not a resource type valid for this command (currently just album)"
    ))]
    UrlForUnsupportedSpotifyResource { got: SpotifyResource<'static> },

    #[snafu(display("couldn't authenticate with Spotify"))]
    SpotifyTokenError { source: rspotify::ClientError },

    #[snafu(display("couldn't retrieve album data from Spotify"))]
    FetchSpotifyAlbumError { source: rspotify::ClientError },

    #[snafu(display("couldn't retrieve data for tracks in this album from Spotify"))]
    FetchSpotifyAlbumTracksError { source: rspotify::ClientError },
}

const COLOR_RED_500: u32 = 0xef4444;
const COLOR_TEAL_500: u32 = 0x14b8a6;
const COLOR_PINK_500: u32 = 0xec4899;

const COLOR_ERROR: u32 = COLOR_RED_500;
// const COLOR_SUCCESS: u32 = COLOR_TEAL_500;
const COLOR_SUCCESS: u32 = COLOR_PINK_500;

impl From<HandleError> for InteractionResponse {
    fn from(error: HandleError) -> Self {
        let embed = EmbedBuilder::new()
            .color(COLOR_ERROR)
            .title("Error")
            .description(Report::from_error(error).to_string())
            .footer(EmbedFooterBuilder::new("Please report this to J / Navith!").build())
            .build();

        let interaction_response_data = InteractionResponseDataBuilder::new()
            .embeds([embed])
            .flags(MessageFlags::EPHEMERAL)
            .build();

        InteractionResponse {
            kind: InteractionResponseType::ChannelMessageWithSource,
            data: Some(interaction_response_data),
        }
    }
}

fn format_or_role(name: &str, roles: &BTreeMap<CaseInsensitiveString, Role>) -> String {
    match roles.get(&CaseInsensitiveString(name.into())) {
        Some(role) => format!("<@&{}>", role.id),
        None => format!("**{name}**"),
    }
}

#[tracing::instrument(ret)]
async fn handle_impl(
    State {
        discord_client,
        spotify_client,
        ..
    }: State,
    interaction: Interaction,
) -> Result<InteractionResponse, HandleError> {
    let guild_id = interaction.guild_id.context(NotUsedInGuildSnafu)?;

    let roles = discord_client
        .roles(guild_id)
        .await
        .context(GetRolesSnafu)?
        .models()
        .await
        .context(DeserializeRolesSnafu)?;

    let roles_map = BTreeMap::from_iter(
        roles
            .into_iter()
            .map(|role| (CaseInsensitiveString((&role.name).into()), role)),
    );

    let InteractionData::ApplicationCommand(command_data) = interaction.data.unwrap() else {
        panic!(
            "this is a command handler so it should be impossible for the interaction data not to be for an application command invocation"
        );
    };
    let command_data = *command_data;

    let mut options = BTreeMap::from_iter(
        command_data
            .options
            .into_iter()
            .map(|CommandDataOption { name, value }| (name, value)),
    );

    let url_command_option_value = options.remove("url").context(UrlMissingSnafu)?;
    let url = match url_command_option_value {
        CommandOptionValue::String(url) => url,
        other => {
            return Err(HandleError::UrlNotString { actual: other });
        }
    };
    let url = IriRefBuf::new(url).context(UrlParseSnafu)?;

    let base = Iri::new("https://open.spotify.com").context(SpotifyBaseNotValidUrlSnafu)?;

    let spotify_resource = parse_spotify_resource(url.as_iri_ref(), base.as_iri_ref())
        .context(UrlForUnsupportedServiceSnafu)?;

    let album_id = match spotify_resource {
        SpotifyResource::Album { id } => id,
        other => return Err(HandleError::UrlForUnsupportedSpotifyResource { got: other }),
    };

    let needs_refresh = spotify_client
        .token
        .lock()
        .await
        .expect("mutex was poisoned")
        .as_ref()
        .map_or(true, |token| {
            token
                .expires_at
                .map_or(true, |expires_at| expires_at <= chrono::Utc::now())
        });

    if needs_refresh {
        spotify_client
            .request_token()
            .await
            .context(SpotifyTokenSnafu)?;
    }

    let market = None;

    let album_data = spotify_client
        .album(album_id.clone(), market)
        .await
        .context(FetchSpotifyAlbumSnafu)?;

    let mut unique_artist_ids = AHashSet::new();

    let mut main_artist_names = Vec::new();
    for main_artist in album_data.artists {
        if let Some(artist_id) = main_artist.id {
            if unique_artist_ids.insert(artist_id) {
                main_artist_names.push(main_artist.name);
            }
        }
    }

    let all_tracks: Vec<_> = spotify_client
        .album_track(album_id.clone(), market)
        .try_collect()
        .await
        .context(FetchSpotifyAlbumTracksSnafu)?;

    let n_tracks = all_tracks.len();

    let mut additional_artist_names = Vec::new();
    for track_data in all_tracks {
        for track_artist in track_data.artists {
            if let Some(artist_id) = track_artist.id {
                if unique_artist_ids.insert(artist_id) {
                    additional_artist_names.push(track_artist.name);
                }
            }
        }
    }

    let mut release_type = match album_data.album_type {
        AlbumType::Album => Some("LP".to_owned()),
        AlbumType::Compilation => Some("Compilation".to_owned()),
        AlbumType::AppearsOn => Some("Appears On (I don't know what this means lol)".to_owned()),
        AlbumType::Single => None,
    };

    let mut title = album_data.name;
    if let Some(new_title) = title.strip_suffix(" - EP") {
        title = new_title.into();
        release_type = Some("EP".into());
    } else if let Some(new_title) = title.strip_suffix(" EP") {
        title = new_title.into();
        release_type = Some("EP".into());
    }

    let release_date = album_data.release_date;

    let now = OffsetDateTime::now_utc();
    let almost_midnight_today = now.replace_time(Time::MAX);

    let release_date = parse_date(&release_date)
        .ok()
        .and_then(|(year, month_and_day)| {
            month_and_day
                .and_then(Result::ok)
                .and_then(|(month, day)| day.and_then(Result::ok).map(|day| (year, month, day)))
        });

    let release_date = release_date.map(|(year, month, day)| {
        let as_date = Date::from_calendar_date(
            year as i32,
            month
                .get()
                .try_into()
                .expect("month is in the range of 1 to 12"),
            day.get(),
        ).expect("I don't know how this failed and it's inconvenient to report the error to the user for now");

        let release_datetime = OffsetDateTime::new_utc(as_date, Time::MIDNIGHT);

        // or time to release if it's negative
        let time_since_release = almost_midnight_today - release_datetime;

        if time_since_release < time::Duration::weeks(52) {
            format!("{month}/{day}")
        } else {
            format!("{year}/{month}/{day}")
        }
    });

    let label = album_data.label;

    let mut release_name = format!("{title}");
    if let Some(release_type) = release_type {
        let release_type_and_tracks = format!("{release_type}, {n_tracks} tracks");

        release_name = format!("{release_name} ({release_type_and_tracks})");
    }

    let url = album_id.url();
    let mut first_line = format!("[{release_name}](<{url}>)");

    if main_artist_names != vec!["Various Artists".to_string()] {
        first_line = format!(
            "{} - {first_line}",
            main_artist_names
                .into_iter()
                .map(|name| format_or_role(&name, &roles_map))
                .join(" & ")
        );
    }
    if let Some(label) = label {
        if roles_map.contains_key(&CaseInsensitiveString((&label).into())) {
            first_line = format!("{first_line} (on {})", format_or_role(&label, &roles_map))
        }
    }
    if let Some(release_date) = release_date {
        first_line = format!("{first_line} [{release_date}]");
    }

    let additional_artist_names = NonEmptyVec::from_vec(additional_artist_names);
    let additional_artists_and_pings = additional_artist_names.map(|names| {
        names
            .into_iter()
            .map(|name| format_or_role(&name, &roles_map))
            .join(", ")
    });
    let second_line = additional_artists_and_pings.map(|s| format!("with {s}"));

    let message = [Some(first_line), second_line]
        .into_iter()
        .flatten()
        .join("\n");

    let copyable = format!("```\n{message}\n```");

    let interaction_response_data = InteractionResponseDataBuilder::new()
        .content("Copy the `Content` and edit it to your liking, then post it yourself. The bot doesn't post for you because it needs human understanding of the release and not just pure data. And, you don't have to use this bot if you don't want to.")
        .embeds([
            EmbedBuilder::new()
                .color(COLOR_SUCCESS)
                .title("Content")
                .description(copyable)
                .build(),

            EmbedBuilder::new()
                .title("Preview")
                .description(message)
                .build(),
        ])
        .flags(MessageFlags::EPHEMERAL)
        .build();

    Ok(InteractionResponse {
        kind: InteractionResponseType::ChannelMessageWithSource,
        data: Some(interaction_response_data),
    })
}

#[tracing::instrument]
pub async fn handle(state: State, interaction: Interaction) -> InteractionResponse {
    match handle_impl(state, interaction).await {
        Ok(interaction_response) => interaction_response,
        Err(error) => error.into(),
    }
}
