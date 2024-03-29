use std::collections::VecDeque;
use std::process::Stdio;
use tokio::process::Child as ChildProcess;
use tokio::process::Command;

use crate::types::ResultAnyError;
use crate::utils;
use anyhow::anyhow;

/// A command that has some presets such as:
/// - Working directory
pub struct PresetCommand {
  pub working_dir: String,
}

impl PresetCommand {
  pub async fn exec(&self, command_str: &str) -> ResultAnyError<String> {
    let command_result = self
      .spawn_command_from_str(command_str, None, None)
      .await?
      .wait_with_output()
      .await?;

    if !command_result.stderr.is_empty() {
      return stderr_to_err(command_result.stderr);
    }

    return utils::bytes_to_string(command_result.stdout);
  }

  pub async fn spawn_command_from_str(
    &self,
    command_str: &str,
    stdin: Option<Stdio>,
    stdout: Option<Stdio>,
  ) -> ResultAnyError<ChildProcess> {
    let mut command_parts: VecDeque<String> =
      PresetCommand::create_command_parts_from_string(command_str);

    let command = command_parts
      .pop_front()
      .ok_or(anyhow!("Invalid command: {}", command_str))?;

    let handle = Command::new(command)
      .args(command_parts)
      .current_dir(&self.working_dir)
      .stdin(stdin.unwrap_or(Stdio::piped()))
      .stdout(stdout.unwrap_or(Stdio::piped()))
      .spawn()?;

    return Ok(handle);
  }
}

impl PresetCommand {
  /// As of now this function does not work for param value that contains
  /// whitespace, for example: `git log --oneline --pretty='format:%h %s'`
  /// the `--pretty='format:%h %s` will fail.
  fn create_command_parts_from_string(command_str: &str) -> VecDeque<String> {
    let command_parts_raw: Vec<String> = command_str.split(' ').map(String::from).collect();
    let mut command_parts: VecDeque<String> = Default::default();
    let mut has_unpaired_string_quote: bool = false;

    for (_, token) in command_parts_raw.iter().enumerate() {
      if command_parts.len() > 1 && has_unpaired_string_quote {
        let previous_token = command_parts.pop_back().unwrap();
        let previous_token = format!("{} {}", previous_token, token);

        command_parts.push_back(previous_token);

        if token.contains("\"") {
          has_unpaired_string_quote = false;
        }
      } else {
        if has_unpaired_string_quote == false && token.contains("\"") {
          has_unpaired_string_quote = true;
        }

        command_parts.push_back(token.to_owned());
      }
    }

    return command_parts;
  }
}

pub fn stderr_to_err(stderr: Vec<u8>) -> ResultAnyError<String> {
  let output_err = utils::bytes_to_string(stderr)?;

  return Err(anyhow!(output_err));
}

pub fn handle_command_output(output: std::process::Output) -> ResultAnyError<String> {
  if !output.stderr.is_empty() {
    // Convert explicitly to Err.
    return stderr_to_err(output.stderr);
  }

  return utils::bytes_to_string(output.stdout);
}

#[cfg(test)]
mod test {
  use super::*;

  mod create_command_parts_from_string {
    use super::*;

    // Deliberately comment it out
    // #[test]
    // fn it_should_parse_string_params_containing_space() {
    //   // 1 space
    //   let command_parts: VecDeque<String> = PresetCommand::create_command_parts_from_string(
    //     "git log --oneline --pretty='format:%h %s'",
    //   );

    //   assert_eq!(
    //     vec![
    //       "git".to_owned(),
    //       "log".to_owned(),
    //       "--oneline".to_owned(),
    //       "--pretty='format:%h %s'".to_owned()
    //     ],
    //     command_parts.into_iter().collect::<Vec<String>>()
    //   );

    //   // 2 spaces
    //   let command_parts: VecDeque<String> =
    //     PresetCommand::create_command_parts_from_string("grep 'Merge pull request' --invert-match");

    //   assert_eq!(
    //     vec![
    //       "grep".to_owned(),
    //       "'Merge pull request'".to_owned(),
    //       "--invert-match".to_owned(),
    //     ],
    //     command_parts.into_iter().collect::<Vec<String>>()
    //   );
    // }

    #[test]
    fn it_should_parse_string_params() {
      let command_parts: VecDeque<String> =
        PresetCommand::create_command_parts_from_string("git log --oneline --pretty=format:%s");

      assert_eq!(
        vec![
          "git".to_owned(),
          "log".to_owned(),
          "--oneline".to_owned(),
          "--pretty=format:%s".to_owned()
        ],
        command_parts.into_iter().collect::<Vec<String>>()
      );
    }
  }
}
