use std::fs;

use clap::App as Cli;
use clap::Arg;
use clap::ArgMatches;
use clap::SubCommand;

use lezeh_common::types::ResultAnyError;

pub mod built_info {
  include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

pub struct BillCli {}

impl BillCli {
  pub fn cmd<'a, 'b>(cli_name: Option<&str>) -> Cli<'a, 'b> {
    return Cli::new(cli_name.unwrap_or("lezeh-bill"))
      .version(built_info::PKG_VERSION)
      .author(built_info::PKG_AUTHORS)
      .about(built_info::PKG_DESCRIPTION)
      .setting(clap::AppSettings::ArgRequiredElseHelp)
      .about("CLI related with bill data processing. Mostly for personal use")
      .subcommand(
        SubCommand::with_name("cc-beautify")
          .about("Beautify the given cc bill")
          .arg(
            Arg::with_name("filepath")
              .required(true)
              .help("Filepath to cc bill"),
          ),
      );
  }

  pub fn run(cli: &ArgMatches<'_>) -> ResultAnyError<()> {
    match cli.subcommand() {
      ("cc-beautify", Some(cc_beautify_cli)) => {
        let filepath: String = cc_beautify_cli.value_of("filepath").unwrap().to_owned();

        return BillCli::cc_beautify(filepath);
      }
      _ => Ok(()),
    }
  }

  pub fn cc_beautify(filepath: String) -> ResultAnyError<()> {
    let file_content: String = std::str::from_utf8(&fs::read(filepath)?[..])?.to_owned();

    let lines: Vec<Vec<String>> = file_content
      .split('\n')
      .into_iter()
      .map(|line| {
        let mut cells: Vec<String> = line.split(' ').into_iter().map(ToOwned::to_owned).collect();

        let mut money_cell: String = cells.pop().unwrap();

        // Payment from last month,
        // we'll just ignore the CR bcs the money cell will be right before it
        if money_cell == "CR" {
          money_cell = cells.pop().unwrap();
        }

        if money_cell.ends_with(".00") {
          money_cell = money_cell.replace(".00", "").replace(",", "");
        } else {
          money_cell = money_cell.replace(".", "");
        }

        cells.push(money_cell);

        return cells;
      })
      .collect();

    let max_cell_count_per_line: usize = lines.iter().map(|iter| iter.len()).max().unwrap();

    let content = lines
      .into_iter()
      .map(|mut cells| {
        if cells.len() < max_cell_count_per_line {
          let money_cell = cells.pop().unwrap();

          let paddings = vec!["-".to_owned(); max_cell_count_per_line - cells.len() - 1];

          cells.extend(paddings);

          cells.push(money_cell);
        }

        return cells.join("\t");
      })
      .collect::<Vec<String>>()
      .join("\n");

    println!("{}", content);

    return Ok(());
  }
}
