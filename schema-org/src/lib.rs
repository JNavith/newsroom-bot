use std::{
    fmt::{self, Display},
    str::FromStr,
};

use iref::IriRefBuf;
use serde_with::serde_as;
use snafu::Snafu;

mod derive_alias {
    derive_aliases::define! {
        Standard = ::std::fmt::Debug, ::core::clone::Clone;
        SchemaOrg = ..Standard, ::serde::Deserialize, ::serde::Serialize;
        SchemaOrgEnum = ..SchemaOrg, ::core::marker::Copy;
    }
}

#[derive_aliases::derive(..SchemaOrg)]
pub struct Thing {
    #[serde(rename = "@id")]
    pub id: Option<IriRefBuf>,

    pub name: Option<Text>,
}

#[derive_aliases::derive(..Standard)]
#[derive(serde_with::DeserializeFromStr, serde_with::SerializeDisplay)]
pub struct Date(pub jiff::civil::Date);

impl FromStr for Date {
    type Err = <jiff::civil::Date as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.parse()?))
    }
}

impl Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive_aliases::derive(..Standard)]
#[derive(serde_with::DeserializeFromStr, serde_with::SerializeDisplay)]
pub struct DateTime(pub chrono::DateTime<chrono::Utc>);

#[derive(Debug, Clone, Snafu)]
pub enum DateTimeParseError {
    /// {original} does not match any expected date formats
    Unmatched { original: String },
}

impl FromStr for DateTime {
    type Err = DateTimeParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok(datetime) = dateparser::parse(s) {
            return Ok(Self(datetime));
        }

        return Err(DateTimeParseError::Unmatched {
            original: s.to_owned(),
        });
    }
}

impl Display for DateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive_aliases::derive(..SchemaOrg)]
#[serde(untagged)]
pub enum DateOrDateTime {
    Date(Date),
    DateTime(DateTime),
}

#[derive_aliases::derive(..SchemaOrg)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    pub birth_date: Option<Date>,

    pub birth_place: Option<Place>,

    #[serde(flatten)]
    pub thing: Thing,
}

#[derive_aliases::derive(..SchemaOrg)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    #[serde(flatten)]
    pub thing: Thing,
}

#[derive_aliases::derive(..SchemaOrg)]
#[serde(rename_all = "camelCase")]
pub struct Organization {
    pub founding_location: Option<Place>,

    #[serde(flatten)]
    pub thing: Thing,
}

impl From<Organization> for Thing {
    fn from(value: Organization) -> Self {
        value.thing
    }
}

#[derive_aliases::derive(..SchemaOrg)]
#[serde(rename_all = "camelCase")]
pub struct PerformingGroup {
    #[serde(flatten)]
    pub organization: Organization,
}

impl From<PerformingGroup> for Thing {
    fn from(value: PerformingGroup) -> Self {
        Self::from(value.organization)
    }
}

pub type Text = String;
pub type URL = IriRefBuf;

#[derive_aliases::derive(..SchemaOrg)]
#[serde(untagged)]
pub enum TextOrURL {
    Text(Text),
    URL(URL),
}

#[derive_aliases::derive(..SchemaOrg)]
#[serde_as]
#[serde(rename_all = "camelCase")]
pub struct MusicGroup {
    #[serde_as(as = "OneOrMany<_>")]
    pub album: Option<Vec<MusicAlbum>>,

    pub genre: Option<TextOrURL>,

    #[serde(flatten)]
    pub performing_group: PerformingGroup,
}

impl From<MusicGroup> for Thing {
    fn from(value: MusicGroup) -> Self {
        Self::from(value.performing_group)
    }
}

// #[derive_aliases::derive(..SchemaOrg)]
// #[serde(untagged)]
// enum PersonOrOrganization {
//     Person(Person),
//     Organization(Organization),
// }

#[derive_aliases::derive(..SchemaOrg)]
#[serde(tag = "@type")]
pub enum SubOfPerson {}

impl From<SubOfPerson> for Thing {
    fn from(value: SubOfPerson) -> Self {
        match value {}
    }
}

#[derive_aliases::derive(..SchemaOrg)]
#[serde(tag = "@type")]
pub enum SubOfPerformingGroup {
    MusicGroup(MusicGroup),
}

impl From<SubOfPerformingGroup> for Thing {
    fn from(value: SubOfPerformingGroup) -> Self {
        match value {
            SubOfPerformingGroup::MusicGroup(music_group) => {
                music_group.performing_group.organization.thing
            }
        }
    }
}

#[derive_aliases::derive(..SchemaOrg)]
#[serde(untagged)]
pub enum PerformingGroupOrSub {
    Sub(SubOfPerformingGroup),
    PerformingGroup(PerformingGroup),
}

impl From<PerformingGroupOrSub> for Thing {
    fn from(value: PerformingGroupOrSub) -> Self {
        match value {
            PerformingGroupOrSub::Sub(sub_of_performing_group) => {
                Self::from(sub_of_performing_group)
            }
            PerformingGroupOrSub::PerformingGroup(performing_group) => Self::from(performing_group),
        }
    }
}

#[derive_aliases::derive(..SchemaOrg)]
#[serde(untagged)]
pub enum SubOfOrganization {
    PerformingGroupOrSub(PerformingGroupOrSub),
}

impl From<SubOfOrganization> for Thing {
    fn from(value: SubOfOrganization) -> Self {
        match value {
            SubOfOrganization::PerformingGroupOrSub(performing_group_or_sub) => {
                Self::from(performing_group_or_sub)
            }
        }
    }
}

#[derive_aliases::derive(..SchemaOrg)]
#[serde(untagged)]
pub enum PersonOrSub {
    Sub(SubOfPerson),
    Person(Person),
}
#[derive_aliases::derive(..SchemaOrg)]
#[serde(untagged)]
pub enum OrganizationOrSub {
    Sub(SubOfOrganization),
    Organization(Organization),
}
#[derive_aliases::derive(..SchemaOrg)]
#[serde(untagged)]
pub enum PersonOrSubOrOrganizationOrSub {
    SubOfPerson(SubOfPerson),
    SubOfOrganization(SubOfOrganization),
    // Person(Person), // TODO: this needs an @type discrimination type of thing
    // Organization(Organization),
}

impl From<PersonOrSubOrOrganizationOrSub> for Thing {
    fn from(value: PersonOrSubOrOrganizationOrSub) -> Self {
        match value {
            PersonOrSubOrOrganizationOrSub::SubOfPerson(sub_of_person) => Self::from(sub_of_person),
            PersonOrSubOrOrganizationOrSub::SubOfOrganization(sub_of_organization) => {
                Self::from(sub_of_organization)
            }
        }
    }
}

#[derive_aliases::derive(..SchemaOrg)]
#[serde(rename_all = "camelCase")]
pub struct CreativeWork {
    pub date_created: Option<DateOrDateTime>,

    pub date_modified: Option<DateOrDateTime>,

    pub date_published: Option<DateOrDateTime>,

    pub publisher: Option<PersonOrSubOrOrganizationOrSub>,

    #[serde(flatten)]
    pub thing: Thing,
}

#[derive_aliases::derive(..SchemaOrg)]
#[serde(rename_all = "camelCase")]
pub struct MusicRecording {
    pub by_artist: Option<MusicGroup>, // TODO: MusicGroupOrPerson

    #[serde(flatten)]
    pub creative_work: CreativeWork,
}

pub type Integer = i64;

#[derive_aliases::derive(..SchemaOrg)]
#[serde_as]
#[serde(rename_all = "camelCase")]
pub struct Intangible {
    #[serde(flatten)]
    pub thing: Thing,
}

#[derive_aliases::derive(..SchemaOrg)]
#[serde_as]
#[serde(rename_all = "camelCase")]
pub struct ListItem<T> {
    pub item: T,

    pub position: Option<Integer>, // TODO: or text

    #[serde(flatten)]
    pub intangible: Intangible,
}

#[derive_aliases::derive(..SchemaOrg)]
#[serde_as]
#[serde(rename_all = "camelCase")]
pub struct ItemList<T> {
    #[serde_as(as = "OneOrMany<_>")]
    pub item_list_element: Vec<ListItem<T>>,

    #[serde(flatten)]
    pub intangible: Intangible,
}

#[derive_aliases::derive(..SchemaOrg)]
#[serde_as]
#[serde(rename_all = "camelCase")]
pub struct MusicPlaylist {
    pub num_tracks: Option<Integer>,

    pub track: Option<ItemList<MusicRecording>>,

    #[serde(flatten)]
    pub creative_work: CreativeWork,
}

#[derive_aliases::derive(..SchemaOrg)]
#[serde(rename_all = "camelCase")]
pub struct MusicRelease {
    pub catalog_number: Option<Text>,

    #[serde(flatten)]
    pub music_playlist: MusicPlaylist,
}

#[derive_aliases::derive(..SchemaOrgEnum)]
pub enum MusicAlbumProductionType {
    CompilationAlbum,
    DJMixAlbum,
    DemoAlbum,
    LiveAlbum,
    MixtapeAlbum,
    RemixAlbum,
    SoundtrackAlbum,
    SpokenWordAlbum,
    StudioAlbum,
}

#[derive_aliases::derive(..SchemaOrgEnum)]
pub enum MusicAlbumReleaseType {
    AlbumRelease,
    BroadcastRelease,
    EPRelease,
    SingleRelease,
}

#[derive_aliases::derive(..SchemaOrg)]
#[serde_as]
#[serde(rename_all = "camelCase")]
pub struct MusicAlbum {
    pub album_production_type: Option<MusicAlbumProductionType>,

    #[serde_as(as = "OneOrMany<_>")]
    pub album_release: Vec<MusicRelease>,

    /// The kind of release which this album is: single, EP or album.
    pub album_release_type: Option<MusicAlbumReleaseType>,

    pub by_artist: Option<MusicGroup>, // TODO: MusicGroupOrPerson

    #[serde(flatten)]
    pub music_playlist: MusicPlaylist,
}
