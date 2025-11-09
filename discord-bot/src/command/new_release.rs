use crate::command::State;
use ahash::AHashSet;
use chrono::Datelike;
use deranged::RangedU8;
use futures::TryStreamExt;
use iref::{
    Iri, IriRef, IriRefBuf,
    iri::{InvalidIriRef, SegmentBuf},
};
use itertools::Itertools;
use nonempty::NonEmpty as NonEmptyVec;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use readformat::readf;
use rspotify::{
    model::{AlbumId, AlbumType, Id, IdError, PlaylistId, SimplifiedArtist, TrackId},
    prelude::BaseClient,
};
use snafu::{OptionExt, Report, ResultExt, Snafu, ensure, futures::TryFutureExt};
use std::{
    collections::{BTreeMap, BTreeSet},
    num::ParseIntError,
    sync::LazyLock,
};
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
    id::marker::GuildMarker,
};
use twilight_util::builder::{
    InteractionResponseDataBuilder,
    command::{CommandBuilder, StringBuilder},
    embed::{EmbedBuilder, EmbedFooterBuilder},
};
use uncased::{Uncased, UncasedStr};

const NAME: &str = "new-release";
const DESCRIPTION: &str = "Post a new music release in this channel";

const URL_NAME: &str = "url";
const URL_DESCRIPTION: &str =
    "The URL to the release on Spotify or Bandcamp (only (known) services supported so far)";

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
    /// this URL isn't one for Spotify that I can recognize
    NotSpotify,

    /// no resource type in the URL
    MissingResourceType,

    /// no resource ID in the URL
    MissingResourceId,

    /// the resource type ({kind:?}) in the URL is not one that I recognize (e.g. album)
    UnrecognizedResourceType { kind: SegmentBuf },

    /// the resource ID in the URL ({id:?}) is not valid by Spotify's rules
    InvalidResourceId { id: String, source: IdError },
}

fn parse_spotify_resource<'a>(
    url: &'a IriRef,
) -> Result<SpotifyResource<'static>, SpotifyResourceFromUrlError> {
    let base = Iri::new("https://open.spotify.com").expect("this is a valid URL");

    ensure!(url.authority() == base.authority(), NotSpotifySnafu);

    let resource_path = url.relative_to(base);

    let mut segments = resource_path.path().segments();

    let kind = segments.next().context(MissingResourceTypeSnafu)?;
    let id = segments.next().context(MissingResourceIdSnafu)?;

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
        _other => Err(SpotifyResourceFromUrlError::UnrecognizedResourceType {
            kind: kind.to_owned(),
        }),
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

const COLOR_RED_500: u32 = 0xef4444;
const COLOR_PINK_500: u32 = 0xec4899;

const COLOR_ERROR: u32 = COLOR_RED_500;
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

#[derive(Debug, Snafu)]
enum GetRolesMapError {
    /// could not fetch the roles in this Discord server
    FetchRolesError { source: twilight_http::Error },

    /// could not deserialize roles in this Discord server after fetching them
    DeserializeRolesError {
        source: twilight_http::response::DeserializeBodyError,
    },
}

#[tracing::instrument(skip(discord_client), ret)]
async fn get_roles_map(
    discord_client: &twilight_http::Client,
    guild_id: twilight_model::id::Id<GuildMarker>,
) -> Result<BTreeMap<Uncased<'static>, Role>, GetRolesMapError> {
    let roles = discord_client
        .roles(guild_id)
        .await
        .context(FetchRolesSnafu)?
        .models()
        .await
        .context(DeserializeRolesSnafu)?;

    Ok(roles
        .into_iter()
        .map(|role| (Uncased::from(role.name.as_str()).into_owned(), role))
        .collect())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReleaseType {
    Single,
    EP,
    LP,
    Compilation,
    Remixes,
    Other(String),
}

#[derive(Debug, Clone)]
struct Artist {
    id: Option<String>, // TODO: I just didn't want to deal with generics
    name: String,
}

#[derive(Debug, Clone)]
struct Track {
    artists: Vec<Artist>,
}

#[derive(Debug, Clone)]
struct Release {
    url: IriRefBuf,
    kind: ReleaseType,
    title: String,
    date: time::Date,
    main_artists: Vec<Artist>,
    tracks: Vec<Track>,
    record_label: Option<String>,
}

#[derive(Debug, Snafu)]
enum GetReleaseFromLdJsonError {
    /// there is no semantic (JSON-LD) release data in the web page (this is likely to mean the service is unsupported)
    NoSemanticDataInPage,

    #[snafu(display(
        "any semantic (JSON-LD) data in the page wasn't able to be parsed (this is likely to mean the service is unsupported): ```{:#?}```",
        Vec::from_iter(errors)
    ))]
    UnsupportedSemanticDataInPage {
        errors: NonEmptyVec<serde_json::Error>, // TODO: it'd be nice if `source` could be a Vec
    },

    /// the semantic data doesn't include a canonical URL to the music release
    NoUrl,

    /// the semantic data doesn't include the type (e.g. EP or LP) of the music release; I don't feel like assuming or guessing (maybe in the future though)
    NoReleaseType,

    /// the semantic data doesn't include the title of the music release
    NoTitle,

    /// the semantic data doesn't include the date of the release
    NoDate,

    /// the semantic data doesn't include the artists of the release
    NoArtists,

    /// the semantic data doesn't include the tracks of the release
    NoTracks,
}

fn get_release_from_ld_json(document: scraper::Html) -> Result<Release, GetReleaseFromLdJsonError> {
    let ld_json_selector = scraper::Selector::parse("script[type='application/ld+json']")
        .expect("ld+json selector should be valid");
    let ld_json_elements = document.select(&ld_json_selector);
    let ld_json_texts = ld_json_elements.map(|e| e.text());
    let ld_json_strings = ld_json_texts.map(String::from_iter);
    let ld_json_strings = Vec::from_iter(ld_json_strings).into_par_iter();
    let music_album_results =
        ld_json_strings.map(|s| serde_json::from_str::<schema_org::MusicAlbum>(&s));

    let (errors, music_albums): (Vec<_>, Vec<_>) = music_album_results.partition_map(Into::into);

    let music_albums_option = NonEmptyVec::from_vec(music_albums);
    let music_albums = match NonEmptyVec::from_vec(errors) {
        Some(errors) => {
            music_albums_option.context(UnsupportedSemanticDataInPageSnafu { errors })?
        }
        None => music_albums_option.context(NoSemanticDataInPageSnafu)?,
    };

    let NonEmptyVec {
        head: first_music_album,
        tail: remaining_music_albums,
    } = music_albums;

    tracing::debug!(
        ?remaining_music_albums,
        "silently ignoring extra music album data in this page"
    );

    let schema_org::MusicAlbum {
        album_release_type,
        by_artist,
        music_playlist,
        ..
    } = first_music_album;
    let schema_org::MusicPlaylist {
        track: tracks,
        creative_work,
        ..
    } = music_playlist;
    let schema_org::CreativeWork {
        date_created,
        date_published,
        publisher,
        thing,
        ..
    } = creative_work;
    let schema_org::Thing { id, name } = thing;

    let url = id.context(NoUrlSnafu)?;

    let release_type = match album_release_type.context(NoReleaseTypeSnafu)? {
        schema_org::MusicAlbumReleaseType::AlbumRelease => ReleaseType::LP,
        schema_org::MusicAlbumReleaseType::BroadcastRelease => {
            ReleaseType::Other("Broadcast (I don't know what this means)".into())
        }
        schema_org::MusicAlbumReleaseType::EPRelease => ReleaseType::EP,
        schema_org::MusicAlbumReleaseType::SingleRelease => ReleaseType::Single,
    };

    let title = name.context(NoTitleSnafu)?;

    let date = date_published.or(date_created).context(NoDateSnafu)?;
    let date = match date {
        schema_org::DateOrDateTime::Date(date) => {
            let jiff_date = date.0;

            let year = jiff_date.year().into();
            let month = u8::try_from(jiff_date.month()).unwrap().try_into().unwrap();
            let day = jiff_date.day().try_into().unwrap();

            time::Date::from_calendar_date(year, month, day)
                .expect("there is simply no way this is an invalid date, I don't buy it")
        }
        schema_org::DateOrDateTime::DateTime(datetime) => {
            let chrono_datetime = datetime.0;

            let year = chrono_datetime.year();
            let month = u8::try_from(chrono_datetime.month())
                .unwrap()
                .try_into()
                .unwrap();
            let day = chrono_datetime.day().try_into().unwrap();

            time::Date::from_calendar_date(year, month, day)
                .expect("there is simply no way this is an invalid date, I don't buy it")
        }
    };

    let main_artists_joined = by_artist.context(NoArtistsSnafu)?;
    let main_artists_joined = main_artists_joined
        .performing_group
        .organization
        .thing
        .name
        .context(NoArtistsSnafu)?;

    let main_artists = parse_list_of_artists(main_artists_joined);
    let main_artists = main_artists.map(|artist_name| Artist {
        id: Some(artist_name.clone()), // sure, why not
        name: artist_name,
    });

    let tracks = tracks
        .context(NoTracksSnafu)?
        .item_list_element
        .into_iter()
        .map(|list_item| list_item.item)
        .map(|music_recording| {
            music_recording
                .by_artist
                .map(schema_org::Thing::from)
                .and_then(|thing| thing.name)
                .map(parse_list_of_artists)
                .map_or_else(
                    || main_artists.clone(),
                    |artists| {
                        artists.map(|artist_name| Artist {
                            id: Some(artist_name.clone()), // sure, why not
                            name: artist_name,
                        })
                    },
                )
        })
        .map(Into::into)
        .map(|artists| Track { artists });
    let tracks = Vec::from_iter(tracks);

    // TODO: do this in a bandcamp-specific way instead
    let release_type = if release_type == ReleaseType::LP {
        if tracks.len() < 3 {
            ReleaseType::Single
        } else if tracks.len() < 7 {
            ReleaseType::EP
        } else {
            ReleaseType::LP // TODO: distinguish compilations
        }
    } else {
        release_type
    };

    let main_artists = main_artists.into();

    let record_label = publisher
        .map(schema_org::Thing::from)
        .and_then(|thing| thing.name);

    Ok(Release {
        url,
        kind: release_type,
        title,
        date,
        main_artists,
        tracks,
        record_label,
    })
}

#[derive(Debug, Snafu)]
enum GetSemanticDataError {
    /// couldn't fetch {url}
    FetchError {
        source: reqwest::Error,
        url: IriRefBuf,
    },
    /// couldn't get the content of the webpage
    ResponseTextError { source: reqwest::Error },

    /// could not surface a release from JSON-LD in the page (this is likely to mean the service is unsupported)
    ReleaseFromLdJsonError { source: GetReleaseFromLdJsonError },
}

#[tracing::instrument(ret)]
async fn get_semantic_data(url: &IriRef) -> Result<Release, GetSemanticDataError> {
    let response = reqwest::get(url.as_str())
        .await
        .with_context(|_| FetchSnafu {
            url: url.to_owned(),
        })?;
    let document = response.text().await.context(ResponseTextSnafu)?;
    let document = scraper::Html::parse_document(&document);

    get_release_from_ld_json(document).context(ReleaseFromLdJsonSnafu)
}

#[derive(Debug, Snafu)]
enum AssembleDateError {
    /// couldn't parse the year
    ParseYearError { source: ParseIntError },

    /// month is missing
    MonthMissing,
    /// couldn't parse the month
    ParseMonthError { source: deranged::ParseIntError },

    /// day is missing
    DayMissing,
    /// couldn't parse the day
    ParseDayError { source: deranged::ParseIntError },

    /// some date component was out of valid range (somehow)
    OutOfRange { source: time::error::ComponentRange },
}

fn assemble_parsed_date(year_result: YearResult) -> Result<time::Date, AssembleDateError> {
    let (year, month_and_day) = year_result.context(ParseYearSnafu)?;
    let (month, day) = month_and_day
        .context(MonthMissingSnafu)?
        .context(ParseMonthSnafu)?;
    let day = day.context(DayMissingSnafu)?.context(ParseDaySnafu)?;

    let date = Date::from_calendar_date(
        year as i32,
        month
            .get()
            .try_into()
            .expect("month is in the range of 1 to 12"),
        day.get(),
    )
    .context(OutOfRangeSnafu)?;

    Ok(date)
}

#[derive(Debug, Snafu)]
enum GetSpotifyReleaseError {
    /// the `url` is for Spotify, but not a resource type valid for this command (currently just album)
    UrlForUnsupportedResource { got: SpotifyResource<'static> },

    /// couldn't authenticate with Spotify
    TokenError { source: rspotify::ClientError },

    /// couldn't retrieve album data from Spotify
    FetchAlbumError { source: rspotify::ClientError },

    /// couldn't retrieve data for tracks in this album from Spotify
    FetchTracksError { source: rspotify::ClientError },

    /// the date of the Spotify release is invalid
    DateInvalid { source: AssembleDateError },

    /// couldn't return a valid URL to the release (for clickability)
    ReturnedUrlInvalid { source: InvalidIriRef<String> },
}

#[tracing::instrument(skip(client), ret)]
async fn get_spotify_release(
    client: &rspotify::ClientCredsSpotify,
    resource: SpotifyResource<'static>,
) -> Result<Release, GetSpotifyReleaseError> {
    let album_id = match resource {
        SpotifyResource::Album { id } => id,
        other => return Err(GetSpotifyReleaseError::UrlForUnsupportedResource { got: other }),
    };

    let needs_refresh = client
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
        client.request_token().await.context(TokenSnafu)?;
    }

    let market = None;

    let (album_data, all_tracks) = tokio::try_join!(
        client
            .album(album_id.as_ref(), market)
            .context(FetchAlbumSnafu),
        client
            .album_track(album_id.as_ref(), market)
            .try_collect::<Vec<_>>()
            .context(FetchTracksSnafu)
    )?;

    let release_type = match album_data.album_type {
        AlbumType::Album => ReleaseType::LP,
        AlbumType::Compilation => ReleaseType::Compilation,
        AlbumType::AppearsOn => {
            ReleaseType::Other("Appears On (I don't know what this means)".into())
        }
        AlbumType::Single => {
            if all_tracks.len() >= 3 {
                ReleaseType::EP
            } else {
                ReleaseType::Single
            }
        }
    };

    fn spotify_artist_to_my_artist_type(spotify_artist: SimplifiedArtist) -> Artist {
        Artist {
            id: spotify_artist.id.as_ref().map(ToString::to_string),
            name: spotify_artist.name,
        }
    }

    let date =
        assemble_parsed_date(parse_date(&album_data.release_date)).context(DateInvalidSnafu)?;

    Ok(Release {
        url: album_id.url().parse().context(ReturnedUrlInvalidSnafu)?,
        kind: release_type,
        title: album_data.name,
        date,
        main_artists: album_data
            .artists
            .into_iter()
            .map(spotify_artist_to_my_artist_type)
            .collect(),
        tracks: all_tracks
            .into_iter()
            .map(|spotify_track| Track {
                artists: spotify_track
                    .artists
                    .into_iter()
                    .map(spotify_artist_to_my_artist_type)
                    .collect(),
            })
            .collect(),
        record_label: album_data.label,
    })
}

#[derive(Debug, Snafu)]
enum GetReleaseError {
    /// could not get release data from Spotify
    SpotifyError { source: GetSpotifyReleaseError },

    /// could not get release data from the web page
    SemanticDataError { source: GetSemanticDataError },
}

#[tracing::instrument(skip(spotify_client), ret)]
async fn get_release(
    spotify_client: &rspotify::ClientCredsSpotify,
    url: IriRefBuf,
) -> Result<Release, GetReleaseError> {
    if let Ok(spotify_resource) = parse_spotify_resource(&url) {
        get_spotify_release(spotify_client, spotify_resource)
            .await
            .context(SpotifySnafu)
    } else {
        get_semantic_data(url.as_iri_ref())
            .await
            .context(SemanticDataSnafu)
    }
}

fn parse_list_of_artists(artists_joined: String) -> NonEmptyVec<String> {
    let artists = NonEmptyVec::collect(artists_joined.rsplit(", ").map(ToOwned::to_owned))
        .expect("rsplit returns at least one thing");

    let last = artists.head;
    let last_ampersand = last.split(" & ").map(ToOwned::to_owned);

    let mut rest = artists.tail;
    rest.reverse();

    NonEmptyVec::collect(rest.into_iter().chain(last_ampersand)).expect(
        "rsplit returned at least one thing earlier, so there is still at least one thing now",
    )
}

fn format_or_role(name: &str, roles_map: &BTreeMap<Uncased, Role>) -> String {
    match roles_map.get(UncasedStr::new(name)) {
        Some(role) => format!("<@&{}>", role.id),
        None => format!("**{name}**"),
    }
}

fn format_release(
    Release {
        url,
        mut kind,
        mut title,
        date,
        main_artists,
        tracks,
        record_label,
    }: Release,
    roles_map: BTreeMap<Uncased<'_>, Role>,
) -> String {
    let mut unique_artist_ids = AHashSet::new();

    let mut main_artist_names = Vec::new();
    for main_artist in main_artists {
        if let Some(artist_id) = main_artist.id {
            if unique_artist_ids.insert(artist_id) {
                main_artist_names.push(main_artist.name);
            }
        }
    }

    let n_tracks = tracks.len();

    let mut additional_artist_names = Vec::new();
    for track in tracks {
        for track_artist in track.artists {
            if let Some(artist_id) = track_artist.id {
                if unique_artist_ids.insert(artist_id) {
                    additional_artist_names.push(track_artist.name);
                }
            }
        }
    }

    // TODO: move this kind of logic out of here because "mutating" release data doesn't fit in with the theme of formatting,
    // and some data providers might already be well-behaved on this front so this should only apply to ones that aren't
    if let Some(new_title) = title.strip_suffix(" - EP") {
        title = new_title.into();
        kind = ReleaseType::EP;
    } else if let Some(new_title) = title.strip_suffix(" (EP)") {
        title = new_title.into();
        kind = ReleaseType::EP;
    } else if let Some(new_title) = title.strip_suffix(" EP") {
        title = new_title.into();
        kind = ReleaseType::EP;
    } else if let Some(new_title) = title.strip_suffix(" - Remixes") {
        title = new_title.into();
        kind = ReleaseType::Remixes;
    } else if let Some(new_title) = title.strip_suffix(" (Remixes)") {
        title = new_title.into();
        kind = ReleaseType::Remixes;
    } else if let Some(new_title) = title.strip_suffix(" Remixes") {
        title = new_title.into();
        kind = ReleaseType::Remixes;
    } else if let Some(new_title) = title.strip_suffix(" - The Remixes") {
        title = new_title.into();
        kind = ReleaseType::Remixes;
    } else if let Some(new_title) = title.strip_suffix(" (The Remixes)") {
        title = new_title.into();
        kind = ReleaseType::Remixes;
    } else if let Some(new_title) = title.strip_suffix(" The Remixes") {
        title = new_title.into();
        kind = ReleaseType::Remixes;
    }

    let release_type = match kind {
        ReleaseType::Single => None,
        ReleaseType::EP => Some("EP".to_owned()),
        ReleaseType::LP => Some("LP".to_owned()),
        ReleaseType::Compilation => Some("Compilation".to_owned()),
        ReleaseType::Remixes => Some("Remixes".to_owned()),
        ReleaseType::Other(s) => Some(s),
    };

    // TODO: move this kind of logic out of here because "mutating" release data doesn't fit in with the theme of formatting,
    // and some data providers might already be well-behaved on this front so this should only apply to ones that aren't
    let (title, features) = match readf("{} (feat. {})", &title) {
        Some(args) => {
            let [title, features] = args.try_into().expect(
                "there should be two things returned because I wrote two {}s in the format string",
            );

            (title, Some(parse_list_of_artists(features)))
        }
        None => (title, None),
    };
    let features_set = features
        .as_ref()
        .map(BTreeSet::from_iter)
        .unwrap_or_default();
    let (title, remixers) = match readf("{} ({} Remix)", &title) {
        Some(args) => {
            let [title, remixers] = args.try_into().expect(
                "there should be two things returned because I wrote two {}s in the format string",
            );

            (title, Some(parse_list_of_artists(remixers)))
        }
        None => (title, None),
    };
    let remixers_set = remixers
        .as_ref()
        .map(BTreeSet::from_iter)
        .unwrap_or_default();

    main_artist_names
        .retain(|artist| !(features_set.contains(artist) || remixers_set.contains(artist)));
    additional_artist_names
        .retain(|artist| !(features_set.contains(artist) || remixers_set.contains(artist)));

    let now = OffsetDateTime::now_utc();
    let almost_midnight_today = now.replace_time(Time::MAX);

    let release_datetime = OffsetDateTime::new_utc(date, Time::MIDNIGHT);

    // or time to release if it's negative
    let time_since_release = almost_midnight_today - release_datetime;

    let year = date.year();
    let month = date.month() as u8;
    let day = date.day();

    let release_date = if time_since_release < time::Duration::weeks(52) {
        format!("{month}/{day}")
    } else {
        format!("{year}/{month}/{day}")
    };

    let mut first_line = format!("[{title}](<{url}>)");

    if let Some(remixers) = remixers {
        let remixers_joined = remixers
            .into_iter()
            .map(|name| format_or_role(&name, &roles_map))
            .join(" & ");

        first_line = format!("{first_line} ({remixers_joined} Remix)");
    }

    if let Some(release_type) = release_type {
        let release_type_and_tracks = format!("{release_type}, {n_tracks} tracks");

        first_line = format!("{first_line} ({release_type_and_tracks})");
    }

    let featured_artists_joined = features.map(|features| {
        features
            .into_iter()
            .map(|name| format_or_role(&name, &roles_map))
            .join(" & ")
    });

    if !main_artist_names.is_empty() && main_artist_names != vec!["Various Artists".to_string()] {
        let main_artists_joined = main_artist_names
            .into_iter()
            .map(|name| format_or_role(&name, &roles_map))
            .join(" & ");
        let mut main_artists_section = main_artists_joined;

        if let Some(featured_artists_joined) = featured_artists_joined {
            main_artists_section =
                format!("{main_artists_section} (feat. {featured_artists_joined})");
        }

        first_line = format!("{main_artists_section} - {first_line}");
    } else if let Some(featured_artists_joined) = featured_artists_joined {
        first_line = format!("{featured_artists_joined} - {first_line}");
    }

    let mut in_brackets = format!("{release_date}");
    if let Some(record_label) = record_label {
        if roles_map.contains_key(UncasedStr::new(&record_label)) {
            let formatted_label = format_or_role(&record_label, &roles_map);
            in_brackets = format!("{in_brackets} on {formatted_label}");
        }
    }

    first_line = format!("{first_line} [{in_brackets}]");

    let additional_artist_names = NonEmptyVec::from_vec(additional_artist_names);
    let additional_artists_and_pings = additional_artist_names.map(|names| {
        names
            .into_iter()
            .map(|name| format_or_role(&name, &roles_map))
            .join(", ")
    });
    let second_line = additional_artists_and_pings.map(|s| format!("with {s}"));

    [Some(first_line), second_line]
        .into_iter()
        .flatten()
        .join("\n")
}

#[derive(Debug, Snafu)]
enum HandleError {
    /// the command was run outside of a Discord server
    NotUsedInGuild,

    /// the `url` argument wasn't provided
    UrlMissing,

    /// the `url` argument wasn't a string like it's supposed to be, it was actually {actual:?}
    UrlNotString { actual: CommandOptionValue },

    /// the `url` argument couldn't be parsed as a URL
    UrlParseError { source: InvalidIriRef<String> },

    /// couldn't get the roles in this server from Discord for pinging purposes
    RolesMapError { source: GetRolesMapError },

    /// couldn't get the release data
    ReleaseError { source: GetReleaseError },
}

#[tracing::instrument(skip(discord_client, spotify_client), ret)]
async fn handle_impl(
    State {
        discord_client,
        spotify_client,
        ..
    }: State,
    interaction: Interaction,
) -> Result<InteractionResponse, HandleError> {
    let guild_id = interaction.guild_id.context(NotUsedInGuildSnafu)?;

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

    let (roles_map, release) = tokio::try_join!(
        get_roles_map(&discord_client, guild_id).context(RolesMapSnafu),
        get_release(&spotify_client, url).context(ReleaseSnafu)
    )?;

    let message = format_release(release, roles_map);
    let copyable = format!("```\n{message}\n```");

    let interaction_response_data = InteractionResponseDataBuilder::new()
        .content("Copy the `Content`, edit it to fix any mistakes, then post it.")
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
