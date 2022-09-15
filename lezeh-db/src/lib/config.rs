use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

use lezeh_common::types::ResultAnyError;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
  pub db_connection_by_name: HashMap<String, DbConnectionConfig>,
}

/// DB Related Command Config
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DbConnectionConfig {
  pub host: String,
  pub port: u32,
  pub database: String,
  pub username: String,
  pub password: Option<String>,
}

impl Config {
  pub fn from(setting_path: impl AsRef<Path> + std::fmt::Display) -> ResultAnyError<Config> {
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
