use super::{Tokens, known_account_id};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

/// These identifiers have different namespaces. Only a server-authenticated
/// observation can associate a streaming username with a Web API account.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct AccountIdentity {
    pub version: u32,
    pub stable_web_api_id: String,
    pub legacy_web_api_id: String,
    pub streaming_username: String,
}

impl Default for AccountIdentity {
    fn default() -> Self {
        Self {
            version: 1,
            stable_web_api_id: String::new(),
            legacy_web_api_id: String::new(),
            streaming_username: String::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AccountRelation {
    Same,
    Different,
    Ambiguous,
}

impl AccountIdentity {
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            bail!(
                "Unsupported saved account identity version; preserve credentials and update Tuitify"
            );
        }
        for id in [
            &self.stable_web_api_id,
            &self.legacy_web_api_id,
            &self.streaming_username,
        ] {
            if id.len() > 512 || id.chars().any(char::is_control) || id.trim() != id {
                bail!(
                    "Invalid saved account identity; credentials and local files have not been changed"
                );
            }
        }
        Ok(())
    }

    pub fn from_profile(profile: &serde_json::Value) -> Result<Self> {
        let field = |name: &str| -> Result<String> {
            match profile.get(name) {
                None | Some(serde_json::Value::Null) => Ok(String::new()),
                Some(serde_json::Value::String(id)) => {
                    Ok(known_account_id(id).unwrap_or_default().to_owned())
                }
                _ => bail!("Spotify account identity response is invalid"),
            }
        };
        let identity = Self {
            stable_web_api_id: field("account_id")?,
            legacy_web_api_id: field("id")?,
            ..Self::default()
        };
        identity.validate()?;
        if known_account_id(&identity.stable_web_api_id).is_none()
            && known_account_id(&identity.legacy_web_api_id).is_none()
        {
            bail!("Spotify account identity missing");
        }
        Ok(identity)
    }

    pub fn relation(&self, other: &Self) -> AccountRelation {
        let compare = |left: &str, right: &str| {
            known_account_id(left)
                .zip(known_account_id(right))
                .map(|(left, right)| {
                    if left == right {
                        AccountRelation::Same
                    } else {
                        AccountRelation::Different
                    }
                })
        };
        // A conflicting stable ID can never be overridden by a legacy alias.
        if let Some(result) = compare(&self.stable_web_api_id, &other.stable_web_api_id) {
            return result;
        }
        compare(&self.legacy_web_api_id, &other.legacy_web_api_id)
            .unwrap_or(AccountRelation::Ambiguous)
    }
}

pub(super) fn token_identity(tokens: &Tokens, streaming: bool) -> Result<AccountIdentity> {
    if let Some(identity) = &tokens.identity {
        identity.validate()?;
        return Ok(identity.clone());
    }
    // Old `account_id` stored /me.id for catalog tokens and the AP username
    // for streaming tokens. It was never Spotify's new immutable account_id.
    let legacy = known_account_id(&tokens.account_id)
        .unwrap_or_default()
        .to_owned();
    Ok(if streaming {
        AccountIdentity {
            streaming_username: legacy,
            ..AccountIdentity::default()
        }
    } else {
        AccountIdentity {
            legacy_web_api_id: legacy,
            ..AccountIdentity::default()
        }
    })
}

pub(super) fn require_same(relation: AccountRelation) -> Result<()> {
    match relation {
        AccountRelation::Same => Ok(()),
        AccountRelation::Different => bail!(
            "Spotify accounts do not match; saved credentials and account files have not been replaced. Choose the same account. To deliberately switch accounts, first run tuitify backup FILE, then tuitify logout and authenticate again."
        ),
        AccountRelation::Ambiguous => bail!(
            "Cannot verify the previous Spotify account; saved credentials and account files have not been replaced. Retry verification. If the previous login cannot be recovered, first run tuitify backup FILE, then deliberately run tuitify logout and authenticate again."
        ),
    }
}

pub(super) fn bind_streaming(
    mut catalog: AccountIdentity,
    authenticated_catalog_username: &str,
    authenticated_streaming_username: &str,
) -> Result<AccountIdentity> {
    let relation = match (
        known_account_id(authenticated_catalog_username),
        known_account_id(authenticated_streaming_username),
    ) {
        (Some(left), Some(right)) if left == right => AccountRelation::Same,
        (Some(_), Some(_)) => AccountRelation::Different,
        _ => AccountRelation::Ambiguous,
    };
    require_same(relation)?;
    catalog.streaming_username = authenticated_streaming_username.to_owned();
    catalog.validate()?;
    Ok(catalog)
}
