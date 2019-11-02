use sled;
use bincode;
use crate::databases;

pub type DResult<T, E = DataserverError> = Result<T, E>;

#[derive(Debug)]
pub enum DataserverError {
    DatabaseError(sled::Error),
    DatabaseLoadError(databases::LoadDbError),
    UserDatabaseError(databases::UserDbError),
    SerializationError(bincode::Error),
    TelegramBotError(telegram_bot::Error)
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