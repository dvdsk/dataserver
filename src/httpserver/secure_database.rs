use crate::httpserver::timeseries_interface;
use serde::{Deserialize,Serialize};
use log::{warn, error};

use sled::{Db,Tree};
use bincode;

use bincode::{deserialize_from,serialize_into};

use ring::{digest, pbkdf2};
use std::collections::HashMap;
use std::fs::{OpenOptions,File};
use std::io::Error as ioError;
use std::path::PathBuf;
use std::num::NonZeroU32;
use std::sync::Arc;

use chrono::{DateTime, Utc};

static DIGEST_ALG: &'static digest::Algorithm = &digest::SHA256;
const CREDENTIAL_LEN: usize = digest::SHA256_OUTPUT_LEN;
const pbkdf2_iterations: NonZeroU32 = unsafe {NonZeroU32::new_unchecked(100_000)};
const db_salt_component: [u8; 16] = [ // This value was generated from a secure PRNG. //TODO check this
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

enum LoadDbError {
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
	pub fn from_db(db: Db) -> Result<Self,sled::Error> {
		Ok(Self { 
			storage: db.open_tree("passw_database")?, //created it not exist
		})
	}
	
	pub fn set_password(&mut self, username: &[u8], password: &[u8])
	 -> Result<(), sled::Error> {
		let salt = self.salt(username);
		let mut credential = [0u8; CREDENTIAL_LEN];
		pbkdf2::derive(DIGEST_ALG, pbkdf2_iterations, &salt, password, &mut credential);
		
		self.storage.set(username, &credential)?;
		self.storage.flush();
		Ok(())
	}

	pub fn verify_password(&self, username: &[u8], attempted_password: &[u8])
	-> Result<(), PasswDbError> {
		if let Ok(db_entry) = self.storage.get(username) {
			if let Some(credential) = db_entry {
				let salted_attempt = self.salt(username);
			 	pbkdf2::verify(DIGEST_ALG, pbkdf2_iterations, &salted_attempt,
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
		let mut salt = Vec::with_capacity(db_salt_component.len() + username.len());
		salt.extend(db_salt_component.as_ref());
		salt.extend(username);
		salt
	}
}

/////////////////////////////////////////////////////////////////////////////////
 
#[derive(Debug, Clone)]
pub struct UserDatabase {
    pub storage: Arc<Tree>,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct UserInfo {
	pub timeseries_with_access: HashMap<timeseries_interface::DatasetId, Vec<timeseries_interface::Authorisation>>,
	pub last_login: DateTime<Utc>, 
	pub username: String,
}

#[derive(Debug)]
pub enum UserDbError {
	UserNotInDb,
	Internal,
}

impl UserDatabase {
	pub fn from_db(db: Db) -> Result<Self,sled::Error> {
		Ok(Self { 
			storage: db.open_tree("user_database")?, //created it not exist
		})
	}

	pub fn get_userdata<T: AsRef<[u8]>>(&self, username: T) -> Result<UserInfo, UserDbError> {
		let username = username.as_ref();
		if let Some(user_data) = self.storage.get(username).map_err(|_| UserDbError::Internal)? {
			let user_info = bincode::deserialize(&user_data).map_err(|_| UserDbError::Internal)?;
			Ok(user_info)
		} else {
			Err(UserDbError::UserNotInDb)
		}
	}
	
	pub fn set_userdata(&mut self, user_info: UserInfo) 
	-> Result <(),UserDbError> {
		let username = user_info.username.as_str().as_bytes();
		let user_data =	bincode::serialize(&user_info).map_err(|_| UserDbError::Internal)?;
		self.storage.set(username,user_data).map_err(|_| UserDbError::Internal)?;
		Ok(())
	}
	
	/*pub fn update_last_login(&mut self, username: &[u8]) {
		match self.storage.remove(username) {
			Some(mut user) => {
				user.user_data.last_login = Utc::now();
				self.storage.insert(Vec::from(username), user);
				self.write();
			},
			None => panic!("user not found in database!"),
		}
	}*/
}
