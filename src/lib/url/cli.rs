use anyhow::anyhow;
use clap::App as Cli;
use clap::Arg;
use clap::ArgMatches;
use clap::SubCommand;

use crate::common::config::Config;
use crate::common::types::ResultAnyError;
use crate::url::client::LezehUrlClient;

pub struct UrlCli {}

impl UrlCli {
  pub fn cmd<'a, 'b>() -> Cli<'a, 'b> {
    return Cli::new("url")
      .setting(clap::AppSettings::ArgRequiredElseHelp)
      .about("url cli")
      .subcommand(
        SubCommand::with_name("shorten")
          .about("Shorten the given url")
          .arg(Arg::with_name("long_url").required(true).help("Long Url")),
      );
  }

  pub async fn run(cli: &ArgMatches<'_>, config: Config) -> ResultAnyError<()> {
    let bitly_config = config.bitly.ok_or(anyhow!("Could not get bitly config"))?;

    let url_client = LezehUrlClient::new(bitly_config);

    if let Some(shorten_cli) = cli.subcommand_matches("shorten") {
      let long_url: &str = shorten_cli.value_of("long_url").unwrap();

      let short_url = url_client.shorten(long_url).await?;

      println!("{}", short_url);
    }

    return Ok(());
  }
}
