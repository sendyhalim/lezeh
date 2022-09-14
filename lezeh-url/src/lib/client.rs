use url::form_urlencoded;

use crate::config::Config;
use lezeh_common::types::ResultAnyError;

pub struct LezehUrlClient {
  config: Config,
}

impl LezehUrlClient {
  pub fn new(config: Config) -> LezehUrlClient {
    return LezehUrlClient { config };
  }
}

impl LezehUrlClient {
  pub async fn shorten(&self, long_url: &str) -> ResultAnyError<String> {
    let encoded_url = form_urlencoded::byte_serialize(long_url.as_bytes()).collect::<String>();

    let bitly_api = format!(
      "https://api-ssl.bitly.com/v3/shorten?access_token={}&longUrl={}&format=txt",
      self.config.bitly.api_token, encoded_url
    );

    let short_url: String = reqwest::get(&bitly_api).await?.text().await?;

    return Ok(short_url);
  }
}
