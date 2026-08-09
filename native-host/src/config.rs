use std::collections::HashSet;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{FcpError, FcpResult};

const BUNDLED_CONFIG: &[u8] = include_bytes!("../../config/account-groups.json");
const MAX_GROUPS: usize = 32;
const MAX_SCOPE_LEN: usize = 253;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountGroupsConfig {
    pub version: u16,
    pub compatibility_version: u16,
    pub groups: Vec<AccountGroup>,
}

/// ADR-020: a group is identified by the registrable domain the user asked to protect. Every
/// cookie under that scope is vaulted; there is no per-site selector or health-check contract.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountGroup {
    pub id: Uuid,
    pub display_name: String,
    pub scope: String,
    pub policy_level: PolicyLevel,
    pub eviction_triggers: Vec<EvictionTrigger>,
    pub store_policy: StorePolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyLevel {
    Critical,
    Balanced,
    Convenient,
    Monitor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyParameters {
    pub hello_cache_ms: Option<u64>,
    pub lease_duration_ms: u64,
    pub idle_threshold_seconds: u64,
    pub last_tab_grace_ms: u64,
    pub monitoring_only: bool,
}

impl PolicyLevel {
    pub fn parameters(self) -> PolicyParameters {
        match self {
            Self::Critical => PolicyParameters {
                hello_cache_ms: Some(0),
                lease_duration_ms: 5 * 60_000,
                idle_threshold_seconds: 60,
                last_tab_grace_ms: 0,
                monitoring_only: false,
            },
            Self::Balanced => PolicyParameters {
                hello_cache_ms: Some(10 * 60_000),
                lease_duration_ms: 10 * 60_000,
                idle_threshold_seconds: 5 * 60,
                last_tab_grace_ms: 2 * 60_000,
                monitoring_only: false,
            },
            Self::Convenient => PolicyParameters {
                // Cleared on lock/disconnect; the 30-minute bound prevents an unbounded grant.
                hello_cache_ms: Some(30 * 60_000),
                lease_duration_ms: 30 * 60_000,
                idle_threshold_seconds: 15 * 60,
                last_tab_grace_ms: 5 * 60_000,
                monitoring_only: false,
            },
            Self::Monitor => PolicyParameters {
                hello_cache_ms: None,
                lease_duration_ms: 0,
                idle_threshold_seconds: 0,
                last_tab_grace_ms: 0,
                monitoring_only: true,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvictionTrigger {
    LastTabClosed,
    Idle,
    Lock,
    Expiry,
    Manual,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorePolicy {
    NormalProfile,
}

#[derive(Clone, Debug)]
pub struct LoadedConfig {
    pub config: AccountGroupsConfig,
    pub digest: String,
}

impl LoadedConfig {
    pub fn load(installed_path: &Path) -> FcpResult<Self> {
        let bytes = if installed_path.exists() {
            fs::read(installed_path)?
        } else {
            BUNDLED_CONFIG.to_vec()
        };
        let config: AccountGroupsConfig = serde_json::from_slice(&bytes)?;
        config.validate()?;
        let digest = encode_hex(&Sha256::digest(&bytes));
        Ok(Self { config, digest })
    }
}

impl AccountGroupsConfig {
    pub fn validate(&self) -> FcpResult<()> {
        if self.version != 2 || self.compatibility_version != 2 {
            return Err(FcpError::Format(
                "unsupported account-group config version".into(),
            ));
        }
        // Zero groups is a valid, expected state pre-launch (2026-08-08): a fresh install starts
        // with none configured and the user adds their first one through onboarding, which
        // requires the host to start successfully before any group exists.
        if self.groups.len() > MAX_GROUPS {
            return Err(FcpError::Format(
                "account-group count is outside bounds".into(),
            ));
        }
        let mut group_ids = HashSet::new();
        let mut scopes: Vec<String> = Vec::new();
        for group in &self.groups {
            if group.id.is_nil() || !group_ids.insert(group.id) {
                return Err(FcpError::Format(
                    "account-group UUID is nil or duplicated".into(),
                ));
            }
            if group.display_name.trim().is_empty() {
                return Err(FcpError::Format(
                    "account-group has an empty required field".into(),
                ));
            }
            let scope = normalize_scope(&group.scope);
            if !is_valid_scope(&scope) {
                return Err(FcpError::Format("account-group scope is invalid".into()));
            }
            // A scope nested inside another would make cookie ownership ambiguous.
            for existing in &scopes {
                if scope == *existing
                    || scope.ends_with(&format!(".{existing}"))
                    || existing.ends_with(&format!(".{scope}"))
                {
                    return Err(FcpError::Format("account-group scopes overlap".into()));
                }
            }
            scopes.push(scope);
            let triggers: HashSet<_> = group.eviction_triggers.iter().copied().collect();
            if group.policy_level != PolicyLevel::Monitor
                && ![
                    EvictionTrigger::LastTabClosed,
                    EvictionTrigger::Idle,
                    EvictionTrigger::Lock,
                    EvictionTrigger::Expiry,
                ]
                .iter()
                .all(|trigger| triggers.contains(trigger))
            {
                return Err(FcpError::Format(
                    "protecting policy omits a mandatory trigger".into(),
                ));
            }
        }
        Ok(())
    }
}

fn normalize_scope(scope: &str) -> String {
    scope.trim_start_matches('.').to_ascii_lowercase()
}

fn is_valid_scope(scope: &str) -> bool {
    // A single label is not a registrable domain: internal page hostnames such as `newtab` must
    // not validate as something the user can protect. `localhost` is the deliberate exception.
    (scope.contains('.') || scope == "localhost")
        && !scope.is_empty()
        && scope.len() <= MAX_SCOPE_LEN
        && !scope.ends_with('.')
        && !scope.contains("..")
        && scope
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_config_is_valid_and_starts_empty() {
        // A real install ships with no pre-seeded sites — Wikipedia/localhost were dev-only
        // testing scopes, removed pre-launch (2026-08-08). Users add their own via the extension.
        let config: AccountGroupsConfig = serde_json::from_slice(BUNDLED_CONFIG).unwrap();
        config.validate().unwrap();
        assert_eq!(config.groups.len(), 0);
    }

    fn group(scope: &str) -> AccountGroup {
        AccountGroup {
            id: Uuid::new_v4(),
            display_name: "test".into(),
            scope: scope.into(),
            policy_level: PolicyLevel::Balanced,
            eviction_triggers: vec![
                EvictionTrigger::LastTabClosed,
                EvictionTrigger::Idle,
                EvictionTrigger::Lock,
                EvictionTrigger::Expiry,
            ],
            store_policy: StorePolicy::NormalProfile,
        }
    }

    fn config_of(groups: Vec<AccountGroup>) -> AccountGroupsConfig {
        AccountGroupsConfig {
            version: 2,
            compatibility_version: 2,
            groups,
        }
    }

    #[test]
    fn nested_scopes_are_rejected_so_cookie_ownership_stays_unambiguous() {
        let config = config_of(vec![group("example.com"), group("mail.example.com")]);
        assert!(config.validate().is_err());
    }

    #[test]
    fn sibling_scopes_are_accepted() {
        let config = config_of(vec![group("example.com"), group("example.org")]);
        config.validate().unwrap();
    }

    #[test]
    fn malformed_scopes_are_rejected() {
        for scope in [
            "",
            "example..com",
            "example.com.",
            "example.com/path",
            "ex ample.com",
            "newtab",
            "extensions",
        ] {
            assert!(
                config_of(vec![group(scope)]).validate().is_err(),
                "scope {scope:?} should be rejected"
            );
        }
    }

    #[test]
    fn policy_idle_thresholds_replace_the_test_constant() {
        assert_eq!(
            PolicyLevel::Critical.parameters().idle_threshold_seconds,
            60
        );
        assert_eq!(
            PolicyLevel::Balanced.parameters().idle_threshold_seconds,
            300
        );
        assert_eq!(
            PolicyLevel::Convenient.parameters().idle_threshold_seconds,
            900
        );
    }
}
