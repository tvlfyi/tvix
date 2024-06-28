use std::{fmt::Display, str::FromStr};

/// Represents configuration as stored in /etc/nix/nix.conf.
/// This list is not exhaustive, feel free to add more.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NixConfig<'a> {
    allowed_users: Option<Vec<&'a str>>,
    auto_optimise_store: Option<bool>,
    cores: Option<u64>,
    max_jobs: Option<u64>,
    require_sigs: Option<bool>,
    sandbox: Option<SandboxSetting>,
    sandbox_fallback: Option<bool>,
    substituters: Option<Vec<&'a str>>,
    system_features: Option<Vec<&'a str>>,
    trusted_public_keys: Option<Vec<crate::narinfo::PubKey>>,
    trusted_substituters: Option<Vec<&'a str>>,
    trusted_users: Option<Vec<&'a str>>,
    extra_platforms: Option<Vec<&'a str>>,
    extra_sandbox_paths: Option<Vec<&'a str>>,
    experimental_features: Option<Vec<&'a str>>,
    builders_use_substitutes: Option<bool>,
}

impl<'a> NixConfig<'a> {
    /// Parses configuration from a file like `/etc/nix/nix.conf`, returning
    /// a [NixConfig] with all values contained in there.
    /// It does not support parsing multiple config files, merging semantics,
    /// and also does not understand `include` and `!include` statements.
    pub fn parse(input: &'a str) -> Result<Self, Error> {
        let mut out = Self::default();

        for line in input.lines() {
            // strip comments at the end of the line
            let line = if let Some((line, _comment)) = line.split_once('#') {
                line
            } else {
                line
            };

            // skip comments and empty lines
            if line.trim().is_empty() {
                continue;
            }

            let (tag, val) = line
                .split_once('=')
                .ok_or_else(|| Error::InvalidLine(line.to_string()))?;

            // trim whitespace
            let tag = tag.trim();
            let val = val.trim();

            #[inline]
            fn parse_val<'a>(this: &mut NixConfig<'a>, tag: &str, val: &'a str) -> Option<()> {
                match tag {
                    "allowed-users" => {
                        this.allowed_users = Some(val.split_whitespace().collect());
                    }
                    "auto-optimise-store" => {
                        this.auto_optimise_store = Some(val.parse::<bool>().ok()?);
                    }
                    "cores" => {
                        this.cores = Some(val.parse().ok()?);
                    }
                    "max-jobs" => {
                        this.max_jobs = Some(val.parse().ok()?);
                    }
                    "require-sigs" => {
                        this.require_sigs = Some(val.parse().ok()?);
                    }
                    "sandbox" => this.sandbox = Some(val.parse().ok()?),
                    "sandbox-fallback" => this.sandbox_fallback = Some(val.parse().ok()?),
                    "substituters" => this.substituters = Some(val.split_whitespace().collect()),
                    "system-features" => {
                        this.system_features = Some(val.split_whitespace().collect())
                    }
                    "trusted-public-keys" => {
                        this.trusted_public_keys = Some(
                            val.split_whitespace()
                                .map(crate::narinfo::PubKey::parse)
                                .collect::<Result<Vec<crate::narinfo::PubKey>, _>>()
                                .ok()?,
                        )
                    }
                    "trusted-substituters" => {
                        this.trusted_substituters = Some(val.split_whitespace().collect())
                    }
                    "trusted-users" => this.trusted_users = Some(val.split_whitespace().collect()),
                    "extra-platforms" => {
                        this.extra_platforms = Some(val.split_whitespace().collect())
                    }
                    "extra-sandbox-paths" => {
                        this.extra_sandbox_paths = Some(val.split_whitespace().collect())
                    }
                    "experimental-features" => {
                        this.experimental_features = Some(val.split_whitespace().collect())
                    }
                    "builders-use-substitutes" => {
                        this.builders_use_substitutes = Some(val.parse().ok()?)
                    }
                    _ => return None,
                }
                Some(())
            }

            parse_val(&mut out, tag, val)
                .ok_or_else(|| Error::InvalidValue(tag.to_string(), val.to_string()))?
        }

        Ok(out)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Invalid line: {0}")]
    InvalidLine(String),
    #[error("Unrecognized key: {0}")]
    UnrecognizedKey(String),
    #[error("Invalid value '{1}' for key '{0}'")]
    InvalidValue(String, String),
}

/// Valid values for the Nix 'sandbox' setting
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SandboxSetting {
    True,
    False,
    Relaxed,
}

impl Display for SandboxSetting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SandboxSetting::True => write!(f, "true"),
            SandboxSetting::False => write!(f, "false"),
            SandboxSetting::Relaxed => write!(f, "relaxed"),
        }
    }
}

impl FromStr for SandboxSetting {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "true" => Self::True,
            "false" => Self::False,
            "relaxed" => Self::Relaxed,
            _ => return Err("invalid value"),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{narinfo::PubKey, nixcpp::conf::SandboxSetting};

    use super::NixConfig;

    #[test]
    pub fn test_parse() {
        let config = NixConfig::parse(include_str!("../../testdata/nix.conf")).expect("must parse");

        assert_eq!(
            NixConfig {
                allowed_users: Some(vec!["*"]),
                auto_optimise_store: Some(false),
                cores: Some(0),
                max_jobs: Some(8),
                require_sigs: Some(true),
                sandbox: Some(SandboxSetting::True),
                sandbox_fallback: Some(false),
                substituters: Some(vec!["https://nix-community.cachix.org", "https://cache.nixos.org/"]),
                system_features: Some(vec!["nixos-test", "benchmark", "big-parallel", "kvm"]),
                trusted_public_keys: Some(vec![
                    PubKey::parse("cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY=")
                        .expect("failed to parse pubkey"),
                    PubKey::parse("nix-community.cachix.org-1:mB9FSh9qf2dCimDSUo8Zy7bkq5CX+/rkCWyvRCYg3Fs=")
                        .expect("failed to parse pubkey")
                ]),
                trusted_substituters: Some(vec![]),
                trusted_users: Some(vec!["flokli"]),
                extra_platforms: Some(vec!["aarch64-linux", "i686-linux"]),
                extra_sandbox_paths: Some(vec![
                    "/run/binfmt", "/nix/store/swwyxyqpazzvbwx8bv40z7ih144q841f-qemu-aarch64-binfmt-P-x86_64-unknown-linux-musl"
                ]),
                experimental_features: Some(vec!["nix-command"]),
                builders_use_substitutes: Some(true)
            },
            config
        );

        // parse a config file using some non-space whitespaces, as well as comments right after the lines.
        // ensure it contains the same data as initially parsed.
        let other_config = NixConfig::parse(include_str!("../../testdata/other_nix.conf"))
            .expect("other config must parse");

        assert_eq!(config, other_config);
    }
}
