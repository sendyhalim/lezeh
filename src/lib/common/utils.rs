use anyhow::Error;

use crate::common::types::ResultAnyError;

pub(crate) fn bytes_to_string(v: Vec<u8>) -> ResultAnyError<String> {
  return std::str::from_utf8(&v)
    .map(String::from)
    .map_err(Error::from);
}
