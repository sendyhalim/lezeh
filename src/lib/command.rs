use std::collections::VecDeque;
use std::process::Command;
use std::process::Stdio;

use crate::types::ResultDynError;
use crate::utils;

/// A command that has some presets such as:
/// - Working directory
pub struct PresetCommand {
  pub working_dir: String,
}

impl PresetCommand {
  pub fn exec(&self, command_str: &str) -> ResultDynError<String> {
    let command_result = self
      .spawn_command_from_str(command_str, None, None)?
      .wait_with_output()?;

    if !command_result.stderr.is_empty() {
      return stderr_to_err(command_result.stderr);
    }

    return utils::bytes_to_string(command_result.stdout);
  }

  pub fn spawn_command_from_str(
    &self,
    command_str: &str,
    stdin: Option<Stdio>,
    stdout: Option<Stdio>,
  ) -> ResultDynError<std::process::Child> {
    let mut command_parts = command_str.split(' ').collect::<VecDeque<&str>>();

    let command = command_parts
      .pop_front()
      .ok_or(format!("Invalid command: {}", command_str))
      .map_err(failure::err_msg)?;

    let handle = Command::new(command)
      .args(command_parts)
      .current_dir(&self.working_dir)
      .stdin(stdin.unwrap_or(Stdio::piped()))
      .stdout(stdout.unwrap_or(Stdio::piped()))
      .spawn()?;

    return Ok(handle);
  }
}

pub fn stderr_to_err(stderr: Vec<u8>) -> ResultDynError<String> {
  let output_err = utils::bytes_to_string(stderr)?;

  return Err(failure::err_msg(output_err));
}

pub fn handle_command_output(output: std::process::Output) -> ResultDynError<String> {
  if !output.stderr.is_empty() {
    // Convert explicitly to Err.
    return stderr_to_err(output.stderr);
  }

  return utils::bytes_to_string(output.stdout);
}
