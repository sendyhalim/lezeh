use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

use lezeh_common::types::ResultAnyError;

/// Phab config
/// -------------
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PhabConfig {
  pub host: String,
  pub api_token: String,
  pub pkcs12_path: String,
  pub pkcs12_password: String,
}

/// Ghub config
/// -------------
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GhubConfig {
  pub api_token: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RepositoryConfig {
  pub key: String,
  pub path: String,
  pub github_path: String, // For example: sendyhalim/foo
  pub deployment_scheme_by_key: HashMap<String, DeploymentSchemeConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeploymentSchemeConfig {
  pub name: String,
  pub default_pull_request_title: String,
  pub merge_from_branch: String,
  pub merge_into_branch: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MergeFeatureBranchesConfig {
  pub output_template_path: Option<String>,
}

impl Default for MergeFeatureBranchesConfig {
  fn default() -> Self {
    return MergeFeatureBranchesConfig {
      output_template_path: Some("merge_feature_branches_default.hbs".to_owned()),
    };
  }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
  pub phab: PhabConfig,
  pub ghub: GhubConfig,
  pub repositories: Vec<RepositoryConfig>,
  pub merge_feature_branches: Option<MergeFeatureBranchesConfig>,
}

impl Config {
  pub fn from(setting_path: impl AsRef<Path> + std::fmt::Display) -> ResultAnyError<Config> {
    let config_str = fs::read_to_string(&setting_path).map_err(|err| {
      return ConfigError::ReadConfigError {
        config_path: setting_path.to_string(),
        root_err: format!("{:#?}", err),
      };
    })?;

    let mut config: Config = serde_yaml::from_str(&config_str).map_err(|err| {
      return ConfigError::ConfigDeserializeError {
        config_path: setting_path.to_string(),
        root_err: format!("{:#?}", err),
      };
    })?;

    if config.merge_feature_branches.is_none() {
      config.merge_feature_branches = Some(Default::default());
    }

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
