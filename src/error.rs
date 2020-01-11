use fern::colors::{Color, ColoredLevelConfig};
use sled;
use bincode;
use crate::databases;

#[derive(Debug)]
pub enum DataserverError {
    DatabaseError(sled::Error),
    DatabaseLoadError(databases::LoadDbError),
    UserDatabaseError(databases::UserDbError),
    SerializationError(bincode::Error),
    //TelegramBotError(telegram_bot::Error)
}

impl From<sled::Error> for DataserverError {
    fn from(error: sled::Error) -> Self {
        DataserverError::DatabaseError(error)
    }
}
impl From<databases::LoadDbError> for DataserverError {
    fn from(error: databases::LoadDbError) -> Self {
        DataserverError::DatabaseLoadError(error)
    }
}
impl From<databases::UserDbError> for DataserverError {
    fn from(error: databases::UserDbError) -> Self {
        DataserverError::UserDatabaseError(error)
    }
}
impl From<bincode::Error> for DataserverError {
    fn from(error: bincode::Error) -> Self {
        DataserverError::SerializationError(error)
    }
}


pub fn setup_logging(verbosity: u8) -> Result<(), fern::InitError> {
	let mut base_config = fern::Dispatch::new();
	let colors = ColoredLevelConfig::new()
	             .info(Color::Green)
	             .debug(Color::Yellow)
	             .warn(Color::Magenta);

	base_config = match verbosity {
		0 =>
			// Let's say we depend on something which whose "info" level messages are too
			// verbose to include in end-user output. If we don't need them,
			// let's not include them.
			base_config
					.level(log::LevelFilter::Error),
		1 =>
			// Let's say we depend on something which whose "info" level messages are too
			// verbose to include in end-user output. If we don't need them,
			// let's not include them.
			base_config
					.level(log::LevelFilter::Warn),
		2 =>
			// Let's say we depend on something which whose "info" level messages are too
			// verbose to include in end-user output. If we don't need them,
			// let's not include them.
			base_config.level(log::LevelFilter::Info)
					.level_for("actix-web", log::LevelFilter::Warn)
					.level_for("dataserver", log::LevelFilter::Trace)
					.level_for("minimal_timeseries", log::LevelFilter::Info),
		3 =>
			// Let's say we depend on something which whose "info" level messages are too
			// verbose to include in end-user output. If we don't need them,
			// let's not include them.
			base_config.level(log::LevelFilter::Trace),
		4 =>
			// Let's say we depend on something which whose "info" level messages are too
			// verbose to include in end-user output. If we don't need them,
			// let's not include them.
			base_config.level(log::LevelFilter::Error),
		_3_or_more => base_config.level(log::LevelFilter::Warn),
	};

	// Separate file config so we can include year, month and day in file logs
	let file_config = fern::Dispatch::new()
		.format(|out, message, record| {
			out.finish(format_args!(
				"{}[{}][{}] {}",
				chrono::Local::now().format("[%Y-%m-%d][%H:%M:%S]"),
				record.target(),
				record.level(),
				message
			))
		})
		.chain(fern::log_file("program.log")?);

	let stdout_config = fern::Dispatch::new()
		.format(move |out, message, record| {
				out.finish(format_args!(
						"[{}][{}][{}] {}",
					chrono::Local::now().format("%H:%M"),
					record.target(),
					colors.color(record.level()),
					message
				))
		})
		.chain(std::io::stdout());

	base_config.chain(file_config).chain(stdout_config).apply()?;
	Ok(())
}