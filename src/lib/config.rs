use std::fs;
use std::path::Path;
use std::{borrow::Cow, collections::HashMap};

use serde::Deserialize;
use serde::Serialize;

use crate::asset::Asset;
use crate::types::ResultDynError;

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

/// Deployment config
/// -------------
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeploymentConfig {
  pub repositories: Vec<RepositoryConfig>,
  pub merge_feature_branches: Option<MergeFeatureBranchesConfig>,
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

/// Bitly config
/// -------------
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BitlyConfig {
  pub api_token: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
  pub phab: PhabConfig,
  pub ghub: GhubConfig,
  pub bitly: Option<BitlyConfig>,
  pub deployment: DeploymentConfig,
}

impl Config {
  pub fn new(setting_path: impl AsRef<Path>) -> ResultDynError<Config> {
    let config_str = fs::read_to_string(setting_path)?;
    let mut config: Config = serde_yaml::from_str(&config_str)?;

    if config.deployment.merge_feature_branches.is_none() {
      config.deployment.merge_feature_branches = Some(Default::default());
    }

    return Ok(config);
  }
}
