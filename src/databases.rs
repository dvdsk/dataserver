use crate::httpserver::timeseries_interface;
use serde::{Deserialize,Serialize};

use sled::{Db,Tree};
use bincode;
use log::error;

use ring::{digest, pbkdf2};
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use telegram_bot::types::refs::UserId as TelegramUserId;

static DIGEST_ALG: &'static digest::Algorithm = &digest::SHA256;
const CREDENTIAL_LEN: usize = digest::SHA256_OUTPUT_LEN;
const PBKDF2_ITERATIONS: NonZeroU32 = unsafe {NonZeroU32::new_unchecked(100_000)};
const DB_SALT_COMPONENT: [u8; 16] = [ // This value was generated from a secure PRNG. //TODO check this
	0xd6, 0x26, 0x98, 0xda, 0xf4, 0xdc, 0x50, 0x52,
	0x24, 0xf2, 0x27, 0xd1, 0xfe, 0x39, 0x01, 0x8a];


pub enum PasswDbError {
	WrongUsername,
	WrongPassword,
	Internal,
}

#[derive(Debug, Clone)]
pub struct PasswordDatabase {
    pub storage: Arc<Tree>,
}

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


impl PasswordDatabase {
	pub fn from_db(db: &Db) -> Result<Self,sled::Error> {
		Ok(Self { 
			storage: db.open_tree("passw_database")?, //created it not exist
		})
	}
	
	pub fn set_password(&mut self, username: &[u8], password: &[u8])
	 -> Result<(), sled::Error> {
		let salt = self.salt(username);
		let mut credential = [0u8; CREDENTIAL_LEN];
		pbkdf2::derive(DIGEST_ALG, PBKDF2_ITERATIONS, &salt, password, &mut credential);
		
		self.storage.set(username, &credential)?;
		self.storage.flush()?;
		Ok(())
	}

	pub fn verify_password(&self, username: &[u8], attempted_password: &[u8])
	-> Result<(), PasswDbError> {
		if let Ok(db_entry) = self.storage.get(username) {
			if let Some(credential) = db_entry {
				let salted_attempt = self.salt(username);
			 	pbkdf2::verify(DIGEST_ALG, PBKDF2_ITERATIONS, &salted_attempt,
					attempted_password,
					&credential)
					.map_err(|_| PasswDbError::WrongPassword)				
			} else { Err(PasswDbError::WrongUsername) }
		} else { Err(PasswDbError::Internal )}
	}

	pub fn is_user_in_database(&self, username: &[u8]) -> Result<bool,sled::Error> {
		self.storage.contains_key(username)
	}

	// The salt should have a user-specific component so that an attacker
	// cannot crack one password for multiple users in the database. It
	// should have a database-unique component so that an attacker cannot
	// crack the same user's password across databases in the unfortunate
	// but common case that the user has used the same password for
	// multiple systems.
	pub fn salt(&self, username: &[u8]) -> Vec<u8> {
		let mut salt = Vec::with_capacity(DB_SALT_COMPONENT.len() + username.len());
		salt.extend(DB_SALT_COMPONENT.as_ref());
		salt.extend(username);
		salt
	}
}

/////////////////////////////////////////////////////////////////////////////////
 
#[derive(Debug, Clone)]
pub struct WebUserDatabase {
    pub storage: Arc<Tree>,
}

type RecieveErrors = bool;
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct WebUserInfo {
	pub timeseries_with_access: HashMap<timeseries_interface::DatasetId, Vec<timeseries_interface::Authorisation>>,
	pub last_login: DateTime<Utc>, 
	pub username: String,
	pub telegram_user_id: Option<TelegramUserId>,
}

#[derive(Debug)]
pub enum UserDbError {
	UserNotInDb,
	DatabaseError(sled::Error),
	SerializeError(bincode::Error),
}

impl From<sled::Error> for UserDbError {
    fn from(error: sled::Error) -> Self {
        UserDbError::DatabaseError(error)
    }
}
impl From<bincode::Error> for UserDbError {
    fn from(error: bincode::Error) -> Self {
        UserDbError::SerializeError(error)
    }
}


impl WebUserDatabase {
	pub fn from_db(db: &Db) -> Result<Self,sled::Error> {
		Ok(WebUserDatabase { 
			storage: db.open_tree("web_user_database")?, //created it not exist
		})
	}

	pub fn get_userdata<T: AsRef<[u8]>>(&self, username: T) -> Result<WebUserInfo, UserDbError> {
		let username = username.as_ref();
		if let Some(user_data) = self.storage.get(username)? {
			let user_info = bincode::deserialize(&user_data)?;
			Ok(user_info)
		} else {
			Err(UserDbError::UserNotInDb)
		}
	}
	
	pub fn set_userdata(&self, user_info: WebUserInfo) 
	-> Result <(),UserDbError> {
		let username = user_info.username.as_str().as_bytes();
		let user_data =	bincode::serialize(&user_info)?;
		self.storage.set(username,user_data)?;
		self.storage.flush()?;
		Ok(())
	}
}

#[derive(Debug, Clone)]
pub struct BotUserDatabase {
    pub storage: Arc<Tree>,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct BotUserInfo {
	pub timeseries_with_access: HashMap<timeseries_interface::DatasetId, Vec<timeseries_interface::Authorisation>>,
	pub username: Option<String>,
	pub aliases: HashMap<String, String>,
}

impl BotUserInfo {
	pub fn from_timeseries_access(timeseries_with_access: &HashMap<timeseries_interface::DatasetId, Vec<timeseries_interface::Authorisation>>)
	-> Self {
		Self {
			timeseries_with_access: timeseries_with_access.clone(),
			username: None,
			aliases: HashMap::new(),
		}
	}
}

impl BotUserDatabase {
	pub fn from_db(db: &Db) -> Result<Self,sled::Error> {
		Ok(Self { 
			storage: db.open_tree("bot_user_database")?, //created it not exist
		})
	}

	pub fn get_userdata(&self, user_id: TelegramUserId) -> Result<BotUserInfo, UserDbError> {
		let user_id = &user_id.to_string();
		if let Some(user_data) = self.storage.get(user_id.as_bytes())? {
			let user_info = bincode::deserialize(&user_data)?;
			Ok(user_info)
		} else {
			Err(UserDbError::UserNotInDb)
		}
	}
	
	pub fn set_userdata<U: Into<TelegramUserId>>(&self, user_id: U, user_info: BotUserInfo) 
	-> Result <(),UserDbError> {
		let user_id = &user_id.into().to_string();
		let user_data =	bincode::serialize(&user_info)?;
		self.storage.set(user_id.as_bytes(),user_data)?;
		self.storage.flush()?;
		Ok(())
	}
}

impl UserDbError {
	pub fn to_text(self, user_id: TelegramUserId) -> String {
		match self {
			UserDbError::UserNotInDb => 
				format!("this telegram account may not use this bot, to be able to use this bot add your telegram id: {} to your account", user_id),
			UserDbError::DatabaseError(error) => {
				error!("Error happend in embedded database: {:?}", error);
				format!("apologies, an internal error happend this has been reported and will be fixed as soon as possible")
			}
			UserDbError::SerializeError(error) => {
				error!("Error happend during serialisation for the embedded database: {:?}", error);
				format!("apologies, an internal error happend this has been reported and will be fixed as soon as possible")
			}
		}
	}
}