use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::anyhow;
use lezeh_db::config::Config as DbConfig;
use lezeh_deployment::config::Config as DeploymentConfig;
use lezeh_url::config::Config as UrlConfig;
use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

use lezeh_common::types::ResultAnyError;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
  pub url: Option<UrlConfig>,
  pub deployment: Option<DeploymentConfig>,
  pub db: Option<DbConfig>,
}

impl Config {
  pub fn new(setting_path: impl AsRef<Path> + std::fmt::Display) -> ResultAnyError<Config> {
    let config_str = fs::read_to_string(&setting_path).map_err(|err| {
      return ConfigError::ReadConfigError {
        config_path: setting_path.to_string(),
        root_err: format!("{:#?}", err),
      };
    })?;

    let config: Config = serde_yaml::from_str(&config_str).map_err(|err| {
      return ConfigError::ConfigDeserializeError {
        config_path: setting_path.to_string(),
        root_err: format!("{:#?}", err),
      };
    })?;

    return Ok(config);
  }
}

#[derive(Error, Debug)]
pub enum ConfigError {
  #[error("Failed reading config {config_path} err {root_err}")]
  ReadConfigError {
    config_path: String,
    root_err: String,
  },

  #[error("Could not deserialize config please check {config_path} err {root_err}")]
  ConfigDeserializeError {
    config_path: String,
    root_err: String,
  },
}
