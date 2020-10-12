use slog::Logger;
use std::io::Error as IOError;
use std::io::ErrorKind as IOErrorKind;
use std::io::Write;
use std::sync::Arc;

pub(crate) struct LoggerAsSink {
  logger: Arc<Logger>,
}

impl LoggerAsSink {
  pub(crate) fn new(logger: Arc<Logger>) -> LoggerAsSink {
    return LoggerAsSink {
      logger: logger.clone(),
    };
  }
}

impl Write for LoggerAsSink {
  fn write(&mut self, buf: &[u8]) -> Result<usize, IOError> {
    let text = std::str::from_utf8(buf)
      .map_err(|err_content| return IOError::new(IOErrorKind::Other, err_content))?;

    slog::info!(self.logger, "{}", text);

    return Ok(buf.len());
  }

  fn flush(&mut self) -> Result<(), IOError> {
    return Ok(());
  }
}
