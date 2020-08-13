use config::Config as BaseConfig;
use config::File;
use config::FileFormat;
use serde::Deserialize;
use std::path::Path;

use crate::types::ResultDynError;

#[derive(Debug, Deserialize, Clone)]
pub struct PhabConfig {
  pub host: String,
  pub api_token: String,
  pub pkcs12_path: String,
  pub pkcs12_password: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct GhubConfig {
  pub api_token: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RepositoryConfig {
  pub path: String,
  pub github_path: String, // For example: sendyhalim/foo
}

#[derive(Debug, Deserialize, Clone)]
pub struct DeploymentConfig {
  pub repositories: Vec<RepositoryConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
  pub phab: PhabConfig,
  pub ghub: GhubConfig,
  pub deployment: DeploymentConfig,
}

impl Config {
  pub fn new(setting_path: impl AsRef<Path>) -> ResultDynError<Config> {
    let mut c = BaseConfig::new();

    let file_config = File::new(setting_path.as_ref().to_str().unwrap(), FileFormat::Hjson);

    c.merge(file_config)?;

    return c.try_into().map_err(|err| {
      return failure::err_msg(err.to_string());
    });
  }
}
