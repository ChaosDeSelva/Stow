//! API keys live only in the OS keychain (service `io.github.chaosdeselva.stow`, accounts
//! `anthropic` / `openai`). Keys saved by v0.1 under service `stow` are migrated on read.

use keyring::{Entry, Error as KeyringError};

use crate::models::Provider;
use crate::{Result, StowError};

/// Keychain service name.
pub const SERVICE: &str = "io.github.chaosdeselva.stow";
/// Service name used by v0.1.
const LEGACY_SERVICE: &str = "stow";

/// Which API key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Account {
    /// Anthropic API key.
    Anthropic,
    /// `OpenAI` API key.
    OpenAi,
}

impl Account {
    /// Keychain account name.
    pub fn as_str(self) -> &'static str {
        match self {
            Account::Anthropic => "anthropic",
            Account::OpenAi => "openai",
        }
    }
}

impl std::str::FromStr for Account {
    type Err = StowError;
    fn from_str(s: &str) -> Result<Self> {
        match s.trim() {
            "anthropic" => Ok(Account::Anthropic),
            "openai" => Ok(Account::OpenAi),
            other => Err(StowError::invalid("account", other)),
        }
    }
}

impl From<Provider> for Account {
    fn from(p: Provider) -> Self {
        match p {
            Provider::Anthropic => Account::Anthropic,
            Provider::OpenAi => Account::OpenAi,
        }
    }
}

fn entry(service: &str, account: Account) -> Result<Entry> {
    Entry::new(service, account.as_str()).map_err(map_err)
}

fn map_err(e: KeyringError) -> StowError {
    StowError::Keychain(e.to_string())
}

/// Store (or replace) the key.
pub fn set(account: Account, key: &str) -> Result<()> {
    entry(SERVICE, account)?.set_password(key).map_err(map_err)
}

/// `Ok(None)` when no credential is stored for `account`.
pub fn get(account: Account) -> Result<Option<String>> {
    match entry(SERVICE, account)?.get_password() {
        Ok(k) => return Ok(Some(k)),
        Err(KeyringError::NoEntry) => {}
        Err(e) => return Err(map_err(e)),
    }
    let legacy = entry(LEGACY_SERVICE, account)?;
    match legacy.get_password() {
        Ok(k) => {
            set(account, &k)?;
            if let Err(e) = legacy.delete_credential() {
                tracing::warn!("could not remove migrated legacy key: {e}");
            }
            Ok(Some(k))
        }
        Err(KeyringError::NoEntry) => Ok(None),
        Err(e) => Err(map_err(e)),
    }
}

/// Deleting a missing credential is not an error.
pub fn delete(account: Account) -> Result<()> {
    for service in [SERVICE, LEGACY_SERVICE] {
        match entry(service, account)?.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => {}
            Err(e) => return Err(map_err(e)),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_parses_known_names_only() {
        assert_eq!("anthropic".parse::<Account>().unwrap(), Account::Anthropic);
        assert_eq!(" openai ".parse::<Account>().unwrap(), Account::OpenAi);
        let err = "bard".parse::<Account>().unwrap_err();
        assert!(matches!(
            err,
            StowError::InvalidInput {
                field: "account",
                ..
            }
        ));
        assert_eq!(Account::from(Provider::OpenAi).as_str(), "openai");
        assert_eq!(SERVICE, "io.github.chaosdeselva.stow");
    }
}
