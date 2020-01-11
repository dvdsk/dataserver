use crate::data_store;
use serde::{Deserialize,Serialize};

use sled::{Db,Tree};
use bincode;
use log::error;

use ring::{digest, pbkdf2};
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};
use telegram_bot::types::refs::UserId as TelegramUserId;
use byteorder::{BigEndian, WriteBytesExt};

use crate::data_store::data_router::Alarm;
use crate::error::DataserverError;

static PBKDF2_ALG: pbkdf2::Algorithm = pbkdf2::PBKDF2_HMAC_SHA256;
const CREDENTIAL_LEN: usize = digest::SHA256_OUTPUT_LEN;

pub enum PasswDbError {
	WrongUsername,
	WrongPassword,
	Internal,
}

#[derive(Debug, Clone)]
pub struct PasswordDatabase {
	pbkdf2_iterations: NonZeroU32,
	db_salt_component: [u8; 16],
    pub storage: Tree,
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
			pbkdf2_iterations: NonZeroU32::new(100_000).unwrap(),
			db_salt_component: [
				// This value was generated from a secure PRNG.
				0xd6, 0x26, 0x98, 0xda, 0xf4, 0xdc, 0x50, 0x52,
				0x24, 0xf2, 0x27, 0xd1, 0xfe, 0x39, 0x01, 0x8a
			],
			storage: db.open_tree("passw_database")?, //created it not exist
		})
	}
	
	pub fn set_password(&mut self, username: &[u8], password: &[u8])
	 -> Result<(), sled::Error> {
		let salt = self.salt(username);
		let mut credential = [0u8; CREDENTIAL_LEN];
		pbkdf2::derive(PBKDF2_ALG, self.pbkdf2_iterations, 
			&salt, password, &mut credential);
		
		self.storage.insert(username, &credential)?;
		self.storage.flush()?;
		Ok(())
	}

	pub fn remove_user(&self, username: &[u8]) -> Result<(), DataserverError> {
		self.storage.remove(username)?;
		Ok(())
	}

	pub fn update(&self, old_name: &str, new_name: &str) {
		if old_name == new_name {return; }
		let credential = self.storage.get(old_name.as_bytes()).unwrap().unwrap();
		self.storage.insert(new_name.as_bytes(), credential).unwrap();
	}

	pub fn verify_password(&self, username: &[u8], attempted_password: &[u8])
	-> Result<(), PasswDbError> {
		if let Ok(db_entry) = self.storage.get(username) {
			if let Some(credential) = db_entry {
				let salted_attempt = self.salt(username);
				 pbkdf2::verify(PBKDF2_ALG, self.pbkdf2_iterations, 
					&salted_attempt, attempted_password, &credential)
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
		let mut salt = Vec::with_capacity(self.db_salt_component.len() 
			+ username.len());
		salt.extend(self.db_salt_component.as_ref());
		salt.extend(username);
		salt
	}
}

/////////////////////////////////////////////////////////////////////////////////
pub type Access = HashMap<data_store::DatasetId, Vec<data_store::Authorisation>>;
type RecieveErrors = bool;
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

#[derive(Debug, Clone)]
pub struct UserDatabase {
	pub storage: Tree,
	db: Db,
}

impl UserDatabase {
	pub fn from_db(db: &Db) -> Result<Self,sled::Error> {
		Ok(UserDatabase { 
			storage: db.open_tree("web_user_database")?, //created it not exist
			db: db.clone(),
		})
	}

	pub fn iter(&self) -> impl Iterator<Item = User> {
		let values = self.storage
			.iter()
			.values()
			.filter_map(Result::ok)
			.map(|user| bincode::deserialize(&user))
			.filter_map(Result::ok);
		
		values
	}

	pub fn get_user(&self, id: UserId) -> Result<User, UserDbError> {
		let key = id.to_be_bytes();
		if let Some(user) = self.storage.get(key)? {
			let user = bincode::deserialize(&user)?;
			Ok(user)
		} else {
			Err(UserDbError::UserNotInDb)
		}
	}

	pub async fn set_user(&self, user: User) 
	-> Result <(),UserDbError> {
		let key = user.id.to_be_bytes();
		let user =	bincode::serialize(&user)?;
		self.storage.insert(key,user)?;
		self.storage.flush_async().await?;
		Ok(())
	}

	pub async fn remove_user(&self, id: UserId) -> Result<(), UserDbError> {
		let key = id.to_be_bytes();
		self.storage.remove(key)?;
		self.storage.flush_async().await?;
		Ok(())
	}

	pub async fn new_user(&self, username: String) -> Result<(), UserDbError>{
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
		Ok(())
	}
}

#[derive(Clone)]
pub struct UserLookup {
	pub name_to_id: Arc<RwLock<HashMap<String, UserId>>>,
	bot_id_to_id: Arc<RwLock<HashMap<TelegramUserId, UserId>>>,
}
impl UserLookup {
	pub fn by_name(&self, username: &String)
	 -> Result<UserId, UserDbError> {
		let id = *self.name_to_id.read().unwrap().get(username)
			.ok_or(UserDbError::UserNotInDb)?;
		Ok(id)
	}
	pub fn by_telegram_id(&self, telegram_id: &TelegramUserId)
	 -> Result<UserId, UserDbError> {
		let id = *self.bot_id_to_id.read().unwrap().get(telegram_id)
			.ok_or(UserDbError::UserNotInDb)?;
		Ok(id)
	}

	pub fn update(&self, old_user: &User, new_user: &User){
		if new_user.name != old_user.name {
			let mut name_to_id = self.name_to_id.write().unwrap();
			name_to_id.remove(&old_user.name);
			name_to_id.insert(new_user.name.clone(), new_user.id);
		}
		if new_user.telegram_id != old_user.telegram_id {
			let mut bot_id_to_id = self.bot_id_to_id.write().unwrap();
			if let Some(bot_id) = old_user.telegram_id{
				bot_id_to_id.remove(&bot_id);
			}
			if let Some(bot_id) = new_user.telegram_id{
				bot_id_to_id.insert(bot_id, new_user.id);
			}
		}
	}

	pub fn from_user_db(db: &UserDatabase) -> Result<Self,UserDbError> {
		let mut name_to_id = HashMap::new();
		let mut bot_id_to_id = HashMap::new();
		
		for row in db.storage.iter().values(){
			let user: User = bincode::deserialize(&row?)?;

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

#[derive(Debug, Clone)]
pub struct AlarmDatabase {
	pub db: Db,
	pub storage: Arc<Tree>,
}

//#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub type AlarmList = Vec<Alarm>;

impl AlarmDatabase {
	pub fn from_db(db: &Db) ->Result<Self, sled::Error> {
		Ok(Self { 
			db: db.clone(),
			storage: db.open_tree("alarms")?, //created it not exist
		})
	}

	pub fn get_alarms<T: AsRef<[u8]>>(&self, username: T) -> Result<AlarmList, UserDbError> {
		let username = username.as_ref();

		
		
		if let Some(alarm_list) = self.storage.get(username)? {
			let alarm_list = bincode::deserialize(&alarm_list)?;
			Ok(alarm_list)
		} else {
			Err(UserDbError::UserNotInDb)
		}
	}

	pub fn add_alarms<T: AsRef<[u8]>>(&self, username: T) -> Result<(), sled::Error> {
		let id = self.storage.generate_id()?
		let mut key = username.as_ref().to_vec();
		key.write_u64::<BigEndian>(id).unwrap();


		user_end.push(0); user_end.push(0);


		if let Some(last_entry) = self.storage
			.range(user_start..user_end.as_ref())
			.keys()
			.next_back(){

			let counter = last_entry[0] + last_entry[1]
		}


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