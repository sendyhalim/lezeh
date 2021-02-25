use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;
use serde::Serialize;

use crate::types::ResultDynError;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PhabConfig {
  pub host: String,
  pub api_token: String,
  pub pkcs12_path: String,
  pub pkcs12_password: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GhubConfig {
  pub api_token: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeploymentSchemeConfig {
  pub name: String,
  pub default_pull_request_title: String,
  pub merge_from_branch: String,
  pub merge_into_branch: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RepositoryConfig {
  pub key: String,
  pub path: String,
  pub github_path: String, // For example: sendyhalim/foo
  pub deployment_scheme_by_key: HashMap<String, DeploymentSchemeConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeploymentConfig {
  pub repositories: Vec<RepositoryConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
  pub phab: PhabConfig,
  pub ghub: GhubConfig,
  pub deployment: DeploymentConfig,
}

impl Config {
  pub fn new(setting_path: impl AsRef<Path>) -> ResultDynError<Config> {
    let config_str = fs::read_to_string(setting_path)?;
    let config: Config = serde_yaml::from_str(&config_str)?;

    return Ok(config);
  }
}
