use postgres::config::Config as PsqlConfig;
use postgres::Client as PsqlClient;

use lezeh_common::types::ResultAnyError;

#[derive(thiserror::Error, Debug)]
pub enum PsqlConnectionError {
  #[error("Error when initialization connection {0}")]
  InitializeConnectionError(String),
}

pub struct PsqlConnection {
  client: PsqlClient,
}

pub struct PsqlCreds {
  pub host: String,
  pub database_name: String,
  pub username: String,
  pub password: Option<String>,
}

impl PsqlConnection {
  pub fn new(creds: &PsqlCreds) -> ResultAnyError<PsqlConnection> {
    return Ok(PsqlConnection {
      client: PsqlConfig::new()
        .user(&creds.username)
        .password(
          creds
            .password
            .as_ref()
            .or(Some(&String::from("")))
            .as_ref()
            .unwrap(),
        )
        .host(&creds.host)
        .dbname(&creds.database_name)
        .connect(postgres::NoTls)
        .map_err(|err| {
          return PsqlConnectionError::InitializeConnectionError(err.to_string());
        })?,
    });
  }
}

impl PsqlConnection {
  pub fn get(&mut self) -> &mut PsqlClient {
    return &mut self.client;
  }
}
