use crate::common::types::ResultDynError;

pub(crate) fn bytes_to_string(v: Vec<u8>) -> ResultDynError<String> {
  return std::str::from_utf8(&v)
    .map(String::from)
    .map_err(failure::err_msg);
}
