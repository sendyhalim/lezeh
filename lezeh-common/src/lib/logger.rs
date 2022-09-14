use slog::*;

static mut LOGGER: Option<Logger> = None;

pub fn get() -> &'static Logger {
  unsafe {
    if LOGGER.is_some() {
      return LOGGER.as_ref().unwrap();
    }

    env_logger::init();

    let log_decorator = slog_term::TermDecorator::new().build();
    let log_drain = slog_term::CompactFormat::new(log_decorator).build().fuse();
    let rust_log_val = std::env::var("RUST_LOG").unwrap_or("info".to_owned());
    let log_drain = slog_envlogger::LogBuilder::new(log_drain)
      .parse(&rust_log_val)
      .build();

    let log_drain = slog_async::Async::new(log_drain).build().fuse();

    LOGGER = Some(slog::Logger::root(log_drain, o!()));

    return LOGGER.as_ref().unwrap();
  }
}
