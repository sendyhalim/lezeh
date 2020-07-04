use config::Config as BaseConfig;
use config::ConfigError;
use config::File;
use config::FileFormat;
use serde::Deserialize;
use std::path::Path;

use crate::types::ResultDynError;

#[derive(Debug, Deserialize)]
pub struct PhabConfig {
  pub host: String,
  pub api_token: String,
  pub pkcs12_path: String,
  pub pkcs12_password: String,
}

#[derive(Debug, Deserialize)]
pub struct Config {
  pub phab: PhabConfig,
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
