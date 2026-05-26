use std::fmt::{Display, Formatter};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum IsolationPolicy {
    #[default]
    Host,
    Docker(IsolationProfile),
}

impl IsolationPolicy {
    pub fn is_host(&self) -> bool {
        matches!(self, Self::Host)
    }
}

impl Display for IsolationPolicy {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Host => formatter.write_str("host"),
            Self::Docker(profile) => {
                formatter.write_str("docker")?;
                if let Some(name) = &profile.name {
                    write!(formatter, ":{name}")?;
                }
                Ok(())
            }
        }
    }
}

impl FromStr for IsolationPolicy {
    type Err = IsolationPolicyParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "host" => Ok(Self::Host),
            "docker" => Ok(Self::Docker(IsolationProfile::default())),
            _ => parse_docker_profile(value),
        }
    }
}

fn parse_docker_profile(value: &str) -> Result<IsolationPolicy, IsolationPolicyParseError> {
    let Some(profile) = value.strip_prefix("docker:") else {
        return Err(IsolationPolicyParseError(value.to_owned()));
    };
    if profile.is_empty() {
        return Err(IsolationPolicyParseError(value.to_owned()));
    }
    Ok(IsolationPolicy::Docker(IsolationProfile {
        name: Some(profile.to_owned()),
    }))
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct IsolationProfile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Error)]
#[error("invalid isolation policy {0}; expected host, docker, or docker:PROFILE")]
pub struct IsolationPolicyParseError(pub String);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolation_policy_parses_host_and_docker_profiles() {
        assert_eq!(
            "host".parse::<IsolationPolicy>().unwrap(),
            IsolationPolicy::Host
        );
        assert_eq!(
            "docker".parse::<IsolationPolicy>().unwrap(),
            IsolationPolicy::Docker(IsolationProfile { name: None })
        );
        assert_eq!(
            "docker:locked".parse::<IsolationPolicy>().unwrap(),
            IsolationPolicy::Docker(IsolationProfile {
                name: Some("locked".to_owned())
            })
        );
    }

    #[test]
    fn isolation_policy_rejects_unknown_or_empty_profiles() {
        for value in ["", "sandbox", "docker:"] {
            assert!(
                value.parse::<IsolationPolicy>().is_err(),
                "accepted invalid isolation policy {value}"
            );
        }
    }
}
