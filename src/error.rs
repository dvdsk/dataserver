use sled;
use crate::httpserver::secure_database;

pub type DResult<T, E = DataserverError> = Result<T, E>;

#[derive(Debug)]
pub enum DataserverError {
    DatabaseError(sled::Error),
    DatabaseLoadError(secure_database::LoadDbError),
    UserDatabaseError(secure_database::UserDbError),
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