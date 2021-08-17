mod user;
mod alarm;
mod passw;

pub use alarm::{AlarmDatabase, AlarmDbError, AlarmId};
pub use user::{UserDatabase, UserLookup, User, Access, UserId, UserDbError};
pub use passw::PasswordDatabase;

#[derive(Debug)]
pub enum LoadDbError {
	DatabaseError(sled::Error),
	SerializeError(bincode::Error),
}

impl From<sled::Error> for LoadDbError {
	fn from(error: sled::Error) -> Self {
		LoadDbError::DatabaseError(error)
	}
}
impl From<bincode::Error> for LoadDbError {
	fn from(error: bincode::Error) -> Self {
		LoadDbError::SerializeError(error)
	}
}
