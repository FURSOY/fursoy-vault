use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{FcpError, FcpResult};

const BUNDLED_CONFIG: &[u8] = include_bytes!("../../config/account-groups.json");
const MAX_GROUPS: usize = 32;
const MAX_SELECTORS: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountGroupsConfig {
    pub version: u16,
    pub compatibility_version: u16,
    pub groups: Vec<AccountGroup>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountGroup {
    pub id: Uuid,
    pub display_name: String,
    pub domains: Vec<String>,
    pub navigation_patterns: Vec<String>,
    pub cookie_selectors: Vec<CookieSelector>,
    pub policy_level: PolicyLevel,
    pub eviction_triggers: Vec<EvictionTrigger>,
    pub health_check: HealthCheck,
    pub store_policy: StorePolicy,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CookieSelector {
    pub id: String,
    pub name: String,
    pub domain: String,
    pub path: String,
    pub required_for_enrollment: bool,
    pub url: String,
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HealthCheck {
    pub kind: HealthCheckKind,
    pub origin: String,
    pub path: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthCheckKind {
    WikipediaUserinfo,
    JsonSessionState,
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
        if self.version != 1 || self.compatibility_version != 1 {
            return Err(FcpError::Format(
                "unsupported account-group config version".into(),
            ));
        }
        if self.groups.is_empty() || self.groups.len() > MAX_GROUPS {
            return Err(FcpError::Format(
                "account-group count is outside bounds".into(),
            ));
        }
        let mut group_ids = HashSet::new();
        let mut selector_owners: HashMap<(String, String, String), Uuid> = HashMap::new();
        let mut exact_navigation_hosts: HashMap<String, Uuid> = HashMap::new();
        let mut selector_count = 0usize;
        for group in &self.groups {
            if group.id.is_nil() || !group_ids.insert(group.id) {
                return Err(FcpError::Format(
                    "account-group UUID is nil or duplicated".into(),
                ));
            }
            if group.display_name.trim().is_empty()
                || group.domains.is_empty()
                || group.navigation_patterns.is_empty()
                || group.cookie_selectors.is_empty()
            {
                return Err(FcpError::Format(
                    "account-group has an empty required field".into(),
                ));
            }
            selector_count = selector_count
                .checked_add(group.cookie_selectors.len())
                .ok_or_else(|| FcpError::Format("selector count overflow".into()))?;
            for pattern in &group.navigation_patterns {
                let host = pattern_host(pattern)?;
                if let Some(owner) = exact_navigation_hosts.insert(host, group.id)
                    && owner != group.id
                {
                    return Err(FcpError::Format(
                        "navigation ownership overlaps groups".into(),
                    ));
                }
            }
            let mut selector_ids = HashSet::new();
            for selector in &group.cookie_selectors {
                if selector.id.trim().is_empty()
                    || selector.name.is_empty()
                    || !selector.path.starts_with('/')
                    || !selector.url.contains("://")
                    || !selector_ids.insert(selector.id.as_str())
                {
                    return Err(FcpError::Format(
                        "invalid or duplicated cookie selector".into(),
                    ));
                }
                let identity = (
                    selector.name.clone(),
                    selector.domain.trim_start_matches('.').to_ascii_lowercase(),
                    selector.path.clone(),
                );
                if let Some(owner) = selector_owners.insert(identity, group.id)
                    && owner != group.id
                {
                    return Err(FcpError::Format(
                        "cookie selector belongs to multiple groups".into(),
                    ));
                }
            }
            if !group
                .cookie_selectors
                .iter()
                .any(|selector| selector.required_for_enrollment)
            {
                return Err(FcpError::Format(
                    "group has no required enrollment selector".into(),
                ));
            }
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
        if selector_count > MAX_SELECTORS {
            return Err(FcpError::Format(
                "cookie selector count exceeds limit".into(),
            ));
        }
        Ok(())
    }
}

fn pattern_host(pattern: &str) -> FcpResult<String> {
    let (_, rest) = pattern
        .split_once("://")
        .ok_or_else(|| FcpError::Format("navigation pattern lacks scheme".into()))?;
    let host = rest
        .split('/')
        .next()
        .ok_or_else(|| FcpError::Format("navigation pattern lacks host".into()))?;
    if host.is_empty() {
        return Err(FcpError::Format("navigation pattern has empty host".into()));
    }
    Ok(host.to_ascii_lowercase())
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
    fn bundled_config_is_valid_and_has_two_isolated_groups() {
        let config: AccountGroupsConfig = serde_json::from_slice(BUNDLED_CONFIG).unwrap();
        config.validate().unwrap();
        assert_eq!(config.groups.len(), 2);
        assert_ne!(config.groups[0].id, config.groups[1].id);
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
