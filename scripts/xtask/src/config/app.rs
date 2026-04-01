//! Manifest for applications under anemone-apps.

use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct App {
    pub name: String,
    pub build: Build,
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Build {
    pub workdir: String,

    #[serde(flatten)]
    pub driver: BuildDriver,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "driver")]
pub enum BuildDriver {
    #[serde(rename = "cargo")]
    Cargo(CargoBuild),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CargoBuild {
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub path: String,
}

impl App {
    pub fn from_str(content: &str) -> anyhow::Result<Self> {
        let manifest: App = toml::from_str(content)
            .with_context(|| "Failed to parse app manifest from string content")?;
        Ok(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parsing() {
        let content =
            std::fs::read_to_string("../../conf/app.toml").expect("Failed to read app.toml");
        let app: App = toml::from_str(&content).expect("Failed to parse app.toml");
        println!("{:#?}", app);
    }
}
