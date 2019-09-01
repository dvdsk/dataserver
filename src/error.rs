use sled;
use bincode;
use crate::httpserver::secure_database;
use crate::telegram_bot;

pub type DResult<T, E = DataserverError> = Result<T, E>;

#[derive(Debug)]
pub enum DataserverError {
    DatabaseError(sled::Error),
    DatabaseLoadError(secure_database::LoadDbError),
    UserDatabaseError(secure_database::UserDbError),
    SerializationError(bincode::Error),
    TelegramBotError(telegram_bot::Error)
}

impl From<sled::Error> for DataserverError {
    fn from(error: sled::Error) -> Self {
        DataserverError::DatabaseError(error)
    }
}
impl From<secure_database::LoadDbError> for DataserverError {
    fn from(error: secure_database::LoadDbError) -> Self {
        DataserverError::DatabaseLoadError(error)
    }
}
impl From<secure_database::UserDbError> for DataserverError {
    fn from(error: secure_database::UserDbError) -> Self {
        DataserverError::UserDatabaseError(error)
    }
}
impl From<bincode::Error> for DataserverError {
    fn from(error: bincode::Error) -> Self {
        DataserverError::SerializationError(error)
    }
}