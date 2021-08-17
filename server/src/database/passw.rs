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

impl PasswordDatabase {
	pub fn from_db(db: &Db) -> Result<Self, sled::Error> {
		Ok(Self {
			pbkdf2_iterations: NonZeroU32::new(100_000).unwrap(),
			db_salt_component: [
				// This value was generated from a secure PRNG.
				0xd6, 0x26, 0x98, 0xda, 0xf4, 0xdc, 0x50, 0x52, 0x24, 0xf2, 0x27, 0xd1, 0xfe, 0x39,
				0x01, 0x8a,
			],
			storage: db.open_tree("passw_database")?, //created it not exist
		})
	}

	pub fn set_password(&mut self, username: &[u8], password: &[u8]) -> Result<(), sled::Error> {
		let salt = self.salt(username);
		let mut credential = [0u8; CREDENTIAL_LEN];
		pbkdf2::derive(
			PBKDF2_ALG,
			self.pbkdf2_iterations,
			&salt,
			password,
			&mut credential,
		);

		self.storage.insert(username, &credential)?;
		self.storage.flush()?;
		Ok(())
	}

	pub async fn remove_user(&self, username: &[u8]) -> Result<(), DataserverError> {
		self.storage.remove(username)?;
		self.storage.flush_async().await?;
		Ok(())
	}

	pub fn update(&self, old_name: &str, new_name: &str) {
		if old_name == new_name {
			return;
		}
		let credential = self.storage.get(old_name.as_bytes()).unwrap().unwrap();
		self.storage
			.insert(new_name.as_bytes(), credential)
			.unwrap();
	}

	pub fn verify_password(
		&self,
		username: &[u8],
		attempted_password: &[u8],
	) -> Result<(), PasswDbError> {
		if let Ok(db_entry) = self.storage.get(username) {
			if let Some(credential) = db_entry {
				let salted_attempt = self.salt(username);
				pbkdf2::verify(
					PBKDF2_ALG,
					self.pbkdf2_iterations,
					&salted_attempt,
					attempted_password,
					&credential,
				)
				.map_err(|_| PasswDbError::WrongPassword)
			} else {
				Err(PasswDbError::WrongUsername)
			}
		} else {
			Err(PasswDbError::Internal)
		}
	}

	//pub fn is_user_in_database(&self, username: &[u8]) -> Result<bool,sled::Error> {
	//	self.storage.contains_key(username)
	//}

	// The salt should have a user-specific component so that an attacker
	// cannot crack one password for multiple users in the database. It
	// should have a database-unique component so that an attacker cannot
	// crack the same user's password across databases in the unfortunate
	// but common case that the user has used the same password for
	// multiple systems.
	pub fn salt(&self, username: &[u8]) -> Vec<u8> {
		let mut salt = Vec::with_capacity(self.db_salt_component.len() + username.len());
		salt.extend(self.db_salt_component.as_ref());
		salt.extend(username);
		salt
	}
}
