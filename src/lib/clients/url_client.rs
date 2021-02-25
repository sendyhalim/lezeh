use url::form_urlencoded;

use crate::config::BitlyConfig;
use crate::types::ResultDynError;

pub struct LezehUrlClient {
  config: BitlyConfig,
}

impl LezehUrlClient {
  pub fn new(config: BitlyConfig) -> LezehUrlClient {
    return LezehUrlClient { config };
  }
}

impl LezehUrlClient {
  pub async fn shorten(&self, long_url: &str) -> ResultDynError<String> {
    let encoded_url = form_urlencoded::byte_serialize(long_url.as_bytes()).collect::<String>();

    let bitly_api = format!(
      "https://api-ssl.bitly.com/v3/shorten?access_token={}&longUrl={}&format=txt",
      self.config.api_token, encoded_url
    );

    let short_url: String = reqwest::get(&bitly_api).await?.text().await?;

    return Ok(short_url);
  }
}
