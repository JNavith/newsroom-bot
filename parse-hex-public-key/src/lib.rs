use ed25519_compact::PublicKey;
use hex::FromHexError;
use snafu::{ResultExt, Snafu};
use std::{error::Error, str::FromStr};

#[derive(Debug, Clone)]
pub struct Hex<S>(pub S);

#[derive(Debug, Clone, Snafu)]
pub enum ParseHexError<SE>
where
    SE: Error + 'static,
{
    #[snafu(display("the given string is not represented in hex"))]
    InvalidHex { source: FromHexError },
    #[snafu(display(
        "when decoded from hex, the given string still can't be converted into a valid (instance of the type in question)"
    ))]
    InvalidItem { source: SE },
}

impl<S> FromStr for Hex<S>
where
    S: TryFrom<Vec<u8>>,
    <S as TryFrom<Vec<u8>>>::Error: Error + 'static,
{
    type Err = ParseHexError<<S as TryFrom<Vec<u8>>>::Error>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = hex::decode(s).context(InvalidHexSnafu)?;
        let item = S::try_from(bytes).context(InvalidItemSnafu)?;

        Ok(Self(item))
    }
}

#[derive(Debug, Clone)]
pub struct PublicKeyOrphanRuleAvoidance(pub PublicKey);

impl TryFrom<Vec<u8>> for PublicKeyOrphanRuleAvoidance {
    type Error = ed25519_compact::Error;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        PublicKey::from_slice(&value).map(Self)
    }
}
