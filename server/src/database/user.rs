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

static PBKDF2_ALG: pbkdf2::Algorithm = pbkdf2::PBKDF2_HMAC_SHA256;
const CREDENTIAL_LEN: usize = digest::SHA256_OUTPUT_LEN;


pub type Access = HashMap<data_store::DatasetId, Vec<data_store::Authorisation>>;
pub type UserId = u64;
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct User {
	pub id: UserId,
	pub name: String,
	pub telegram_id: Option<TelegramUserId>,

	pub timeseries_with_access: Access,
	pub last_login: DateTime<Utc>,
	pub aliases: HashMap<String, String>,
	pub keyboard: Option<String>,
	pub timezone_offset: i32, //hours to the east
}

#[derive(ErrorLevel, Error, Debug)]
pub enum UserDbError {
    #[report(no)]
	#[error("this telegram account may not use this bot, to be able to use this bot add your telegram id: {0} to your account")]
	TelegramUserNotInDb(TelegramUserId),
    #[report(no)]
	#[error("I know no user by the name: {0}")]
	UserNameNotInDb(String),
    #[report(no)]
	#[error("No user with id {0} exists in the database")]
	UserNotInDb(UserId),
    #[report(error)]
	#[error("An internal error occured")]
	DatabaseError(#[from] sled::Error),
    #[report(error)]
	#[error("An internal error occured")]
	SerializeError(#[from] bincode::Error),
}

#[derive(Debug, Clone)]
pub struct UserDatabase {
	pub storage: Tree,
	db: Db,
}

impl UserDatabase {
	pub fn from_db(db: &Db) -> Result<Self, sled::Error> {
		Ok(UserDatabase {
			storage: db.open_tree("web_user_database")?, //created it not exist
			db: db.clone(),
		})
	}

	pub fn iter(&self) -> impl Iterator<Item = User> {
		self
			.storage
			.iter()
			.values()
			.filter_map(Result::ok)
			.map(|user| bincode::deserialize(&user))
			.filter_map(Result::ok)
	}

	pub fn get_user(&self, id: UserId) -> Result<User, UserDbError> {
		let key = id.to_be_bytes();
		if let Some(user) = self.storage.get(key)? {
			let user = bincode::deserialize(&user)?;
			Ok(user)
		} else {
			Err(UserDbError::UserNotInDb(id))
		}
	}

	pub async fn set_user(&self, user: User) -> Result<(), UserDbError> {
		let key = user.id.to_be_bytes();
		let user = bincode::serialize(&user)?;
		self.storage.insert(key, user)?;
		self.storage.flush_async().await?;
		Ok(())
	}

	pub async fn remove_user(&self, id: UserId) -> Result<(), UserDbError> {
		let key = id.to_be_bytes();
		self.storage.remove(key)?;
		self.storage.flush_async().await?;
		Ok(())
	}

	pub async fn new_user(&self, username: String) -> Result<UserId, UserDbError> {
		let id = self.db.generate_id()?;

		let user = User {
			id,
			timeseries_with_access: HashMap::new(),
			last_login: Utc::now(),
			name: username,
			telegram_id: None,
			aliases: HashMap::new(),
			keyboard: None,
			timezone_offset: 0, //hours to the east
		};

		self.set_user(user).await?;
		Ok(id)
	}
}

#[derive(Clone)]
pub struct UserLookup {
	pub name_to_id: Arc<RwLock<HashMap<String, UserId>>>,
	bot_id_to_id: Arc<RwLock<HashMap<TelegramUserId, UserId>>>,
}
impl UserLookup {
	pub fn is_unique_telegram_id(&self, id: &TelegramUserId) -> bool {
		!self.bot_id_to_id.read().unwrap().contains_key(id)
	}

	pub fn is_unique_name(&self, name: &str) -> bool {
		!self.name_to_id.read().unwrap().contains_key(name)
	}

	pub fn by_name(&self, username: &str) -> Result<UserId, UserDbError> {
		let id = *self
			.name_to_id
			.read()
			.unwrap()
			.get(username)
			.ok_or_else(|| UserDbError::UserNameNotInDb(username.to_owned()))?;
		Ok(id)
	}
	pub fn by_telegram_id(&self, telegram_id: &TelegramUserId) -> Result<UserId, UserDbError> {
		let id = *self
			.bot_id_to_id
			.read()
			.unwrap()
			.get(telegram_id)
			.ok_or_else(|| UserDbError::TelegramUserNotInDb(*telegram_id))?;
		Ok(id)
	}

	pub fn update(&self, old_user: &User, new_user: &User) {
		if new_user.name != old_user.name {
			let mut name_to_id = self.name_to_id.write().unwrap();
			name_to_id.remove(&old_user.name);
			name_to_id.insert(new_user.name.clone(), new_user.id);
		}
		if new_user.telegram_id != old_user.telegram_id {
			let mut bot_id_to_id = self.bot_id_to_id.write().unwrap();
			if let Some(bot_id) = old_user.telegram_id {
				bot_id_to_id.remove(&bot_id);
			}
			if let Some(bot_id) = new_user.telegram_id {
				bot_id_to_id.insert(bot_id, new_user.id);
			}
		}
	}

	pub fn add(&self, name: String, id: UserId) {
		let mut name_to_id = self.name_to_id.write().unwrap();
		name_to_id.insert(name, id);
	}

	pub fn remove_by_name(&self, name: &str) {
		let mut name_to_id = self.name_to_id.write().unwrap();
		name_to_id.remove(name);
	}

	pub fn from_user_db(db: &UserDatabase) -> Result<Self, UserDbError> {
		let mut name_to_id = HashMap::new();
		let mut bot_id_to_id = HashMap::new();

		for row in db.storage.iter().values() {
			let user: User = bincode::deserialize(&row?)?;
			//dbg!(&user);
			let id = user.id;
			let name = user.name;
			name_to_id.insert(name, id);

			if let Some(bot_id) = user.telegram_id {
				bot_id_to_id.insert(bot_id, id);
			}
		}

		Ok(Self {
			name_to_id: Arc::new(RwLock::new(name_to_id)),
			bot_id_to_id: Arc::new(RwLock::new(bot_id_to_id)),
		})
	}
}
