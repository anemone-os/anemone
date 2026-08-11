//! SystemTarget-owned boot and root selection.
//!
//! The Stage 1B cutover removed the legacy Platform root fields. Production
//! build resolution must use this value and must not recreate a Platform
//! fallback.

use serde::Deserialize;
use std::net::Ipv4Addr;

use super::reference::{AppRef, PlatformRef};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub platform: PlatformRef,
    pub root: Root,
    #[serde(rename = "initial-program")]
    pub initial_program: InitialProgramSource,
    pub network: Option<Network>,
}

impl Config {
    pub fn from_str(content: &str) -> anyhow::Result<Self> {
        let config: Self = toml::from_str(content)?;
        config.root.validate()?;
        config.initial_program.validate()?;
        if let Some(network) = &config.network {
            network.validate()?;
        }
        Ok(config)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Network {
    pub ipv4: StaticIpv4,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StaticIpv4 {
    pub interface: String,
    #[serde(deserialize_with = "deserialize_ipv4")]
    pub address: Ipv4Addr,
    pub prefix: u8,
    #[serde(
        rename = "default-gateway",
        deserialize_with = "deserialize_optional_ipv4",
        default
    )]
    pub default_gateway: Option<Ipv4Addr>,
}

impl Network {
    fn validate(&self) -> anyhow::Result<()> {
        let ipv4 = &self.ipv4;
        if ipv4.interface.is_empty() {
            anyhow::bail!("system target network IPv4 interface must not be empty");
        }
        if ipv4.prefix > 32 {
            anyhow::bail!("system target network IPv4 prefix must not exceed 32");
        }
        validate_external_address(ipv4.address, ipv4.prefix)?;
        if let Some(gateway) = ipv4.default_gateway {
            validate_gateway(gateway)?;
        }
        Ok(())
    }
}

fn deserialize_ipv4<'de, D>(deserializer: D) -> Result<Ipv4Addr, D::Error>
where
    D: serde::Deserializer<'de>,
{
    String::deserialize(deserializer)?
        .parse()
        .map_err(serde::de::Error::custom)
}

fn deserialize_optional_ipv4<'de, D>(deserializer: D) -> Result<Option<Ipv4Addr>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserialize_ipv4(deserializer).map(Some)
}

fn validate_external_address(address: Ipv4Addr, prefix: u8) -> anyhow::Result<()> {
    if address.is_unspecified()
        || address.is_multicast()
        || address.is_broadcast()
        || address.is_loopback()
    {
        anyhow::bail!("system target external IPv4 address must be unicast and non-loopback");
    }

    if prefix < 31 {
        let bits = u32::from(address);
        let mask = if prefix == 0 {
            0
        } else {
            u32::MAX << (32 - prefix)
        };
        let network = bits & mask;
        let broadcast = network | !mask;
        if bits == network || bits == broadcast {
            anyhow::bail!(
                "system target external IPv4 address must not be a subnet network or broadcast address"
            );
        }
    }
    Ok(())
}

fn validate_gateway(gateway: Ipv4Addr) -> anyhow::Result<()> {
    if gateway.is_unspecified()
        || gateway.is_multicast()
        || gateway.is_broadcast()
        || gateway.is_loopback()
    {
        anyhow::bail!("system target IPv4 default gateway must be unicast and non-loopback");
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Root {
    pub fstype: String,
    pub source: RootSource,
}

impl Root {
    fn validate(&self) -> anyhow::Result<()> {
        if self.fstype.is_empty() {
            anyhow::bail!("system target root filesystem type must not be empty");
        }
        if let RootSource::Block { path } = &self.source
            && path.is_empty()
        {
            anyhow::bail!("system target block root path must not be empty");
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum RootSource {
    Block { path: String },
    Pseudo,
}

impl RootSource {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Block { .. } => "block",
            Self::Pseudo => "pseudo",
        }
    }

    pub fn path(&self) -> Option<&str> {
        match self {
            Self::Block { path } => Some(path),
            Self::Pseudo => None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum InitialProgramSource {
    RootfsEntry {
        #[serde(default)]
        argv: Option<Vec<String>>,
    },
    EmbeddedApp {
        app: AppRef,
        #[serde(default)]
        argv: Option<Vec<String>>,
    },
}

impl InitialProgramSource {
    fn validate(&self) -> anyhow::Result<()> {
        let argv = match self {
            Self::RootfsEntry { argv } | Self::EmbeddedApp { argv, .. } => argv,
        };
        if argv.as_ref().is_some_and(Vec::is_empty) {
            anyhow::bail!("initial-program argv must not be empty");
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) const TEST_SYSTEM_TARGET: &str = r#"
platform = "example"

[root]
fstype = "ext4"
source = { type = "block", path = "vda" }

[initial-program]
type = "rootfs-entry"
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rootfs_entry_target() {
        let config = Config::from_str(TEST_SYSTEM_TARGET).unwrap();
        assert_eq!(config.platform.as_str(), "example");
        assert_eq!(config.root.fstype, "ext4");
        assert!(matches!(config.root.source, RootSource::Block { .. }));
        assert!(matches!(
            config.initial_program,
            InitialProgramSource::RootfsEntry { argv: None }
        ));
        assert!(config.network.is_none());
    }

    #[test]
    fn parses_optional_static_ipv4_network() {
        let content = TEST_SYSTEM_TARGET.replace(
            "[initial-program]",
            "[network.ipv4]\ninterface = \"eth0\"\naddress = \"10.0.2.15\"\nprefix = 24\ndefault-gateway = \"10.0.2.2\"\n\n[initial-program]",
        );
        let config = Config::from_str(&content).unwrap();
        let ipv4 = &config.network.unwrap().ipv4;
        assert_eq!(ipv4.interface, "eth0");
        assert_eq!(ipv4.address.octets(), [10, 0, 2, 15]);
        assert_eq!(ipv4.prefix, 24);
        assert_eq!(ipv4.default_gateway.unwrap().octets(), [10, 0, 2, 2]);
    }

    #[test]
    fn rejects_invalid_static_ipv4_network() {
        let valid = TEST_SYSTEM_TARGET.replace(
            "[initial-program]",
            "[network.ipv4]\ninterface = \"eth0\"\naddress = \"10.0.2.15\"\nprefix = 24\n\n[initial-program]",
        );
        for (needle, replacement) in [
            ("\ninterface = \"eth0\"", "\ninterface = \"\""),
            ("\naddress = \"10.0.2.15\"", "\naddress = \"not-an-ip\""),
            ("\naddress = \"10.0.2.15\"", "\naddress = \"0.0.0.0\""),
            ("\naddress = \"10.0.2.15\"", "\naddress = \"127.0.0.1\""),
            ("\naddress = \"10.0.2.15\"", "\naddress = \"224.0.0.1\""),
            ("\naddress = \"10.0.2.15\"", "\naddress = \"10.0.2.255\""),
            ("\nprefix = 24", "\nprefix = 33"),
        ] {
            assert!(
                Config::from_str(&valid.replacen(needle, replacement, 1)).is_err(),
                "accepted invalid replacement: {replacement}"
            );
        }

        let gateway = valid.replace(
            "\nprefix = 24",
            "\nprefix = 24\ndefault-gateway = \"255.255.255.255\"",
        );
        assert!(Config::from_str(&gateway).is_err());

        let unknown = valid.replace("\nprefix = 24", "\nprefix = 24\nroute = []");
        assert!(Config::from_str(&unknown).is_err());
    }

    #[test]
    fn rejects_unsupported_initial_program_tag() {
        let content = TEST_SYSTEM_TARGET.replace("rootfs-entry", "unknown");
        assert!(Config::from_str(&content).is_err());
    }

    #[test]
    fn parses_embedded_app_target() {
        let valid = TEST_SYSTEM_TARGET;
        let content = valid.replace(
            "type = \"rootfs-entry\"",
            "type = \"embedded-app\"\napp = \"init\"",
        );
        let config = Config::from_str(&content).unwrap();
        assert!(matches!(
            config.initial_program,
            InitialProgramSource::EmbeddedApp { app, argv: None } if app.as_str() == "init"
        ));

        let missing_app = valid.replace("type = \"rootfs-entry\"", "type = \"embedded-app\"");
        assert!(Config::from_str(&missing_app).is_err());

        let invalid_app = valid.replace(
            "type = \"rootfs-entry\"",
            "type = \"embedded-app\"\napp = \"../init\"",
        );
        assert!(Config::from_str(&invalid_app).is_err());
    }

    #[test]
    fn initial_program_argv_is_complete_and_nonempty_when_present() {
        let valid = TEST_SYSTEM_TARGET;
        for replacement in [
            "type = \"rootfs-entry\"\nargv = [\"busybox\", \"sh\"]",
            "type = \"embedded-app\"\napp = \"init\"\nargv = [\"init\", \"--test\"]",
        ] {
            let config =
                Config::from_str(&valid.replace("type = \"rootfs-entry\"", replacement)).unwrap();
            let argv = match config.initial_program {
                InitialProgramSource::RootfsEntry { argv }
                | InitialProgramSource::EmbeddedApp { argv, .. } => argv.unwrap(),
            };
            assert_eq!(argv.len(), 2);
        }

        let empty = valid.replace(
            "type = \"rootfs-entry\"",
            "type = \"rootfs-entry\"\nargv = []",
        );
        assert!(Config::from_str(&empty).is_err());
    }

    #[test]
    fn rejects_invalid_root_source() {
        let valid = TEST_SYSTEM_TARGET;
        let empty_fstype = valid.replace("fstype = \"ext4\"", "fstype = \"\"");
        assert!(Config::from_str(&empty_fstype).is_err());

        let empty_block_path = valid.replace("path = \"vda\"", "path = \"\"");
        assert!(Config::from_str(&empty_block_path).is_err());
    }

    #[test]
    fn rejects_fields_owned_by_other_layers() {
        let valid = TEST_SYSTEM_TARGET;
        for field in [
            "preset = \"dev\"",
            "profile = \"release\"",
            "qemu = {}",
            "outputs = []",
        ] {
            let content = valid.replacen(
                "platform = \"example\"",
                &format!("platform = \"example\"\n{field}"),
                1,
            );
            assert!(Config::from_str(&content).is_err(), "{field}");
        }
    }

    #[test]
    fn repository_example_system_target_parses() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../conf/system-targets/example.toml");
        let content = std::fs::read_to_string(path).expect("failed to read example SystemTarget");
        Config::from_str(&content).expect("repository example SystemTarget must parse");
    }
}
