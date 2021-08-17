use crate::data_store;
use serde::{Deserialize, Serialize};
use error_level::ErrorLevel;
use thiserror::Error;

use log::error;
use sled::{Db, Tree};

use ring::{digest, pbkdf2};
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::{Arc, RwLock};

use byteorder::{BigEndian, ByteOrder};
use chrono::{DateTime, Utc};
use telegram_bot::types::refs::UserId as TelegramUserId;

use crate::data_store::data_router::Alarm;
use crate::error::DataserverError;

mod user;
mod alarm;
mod passw;

pub use alarm::AlarmDatabase;
pub use user::{UserDatabase, UserLookup};
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
