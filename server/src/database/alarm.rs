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

#[derive(Debug, Clone)]
pub struct AlarmDatabase {
	pub db: Db,
	pub storage: Tree,
}

#[derive(thiserror::Error, Debug)]
pub enum AlarmDbError {
	#[error("internal database error: {0:?}")]
	DatabaseError(#[from] sled::Error),
	#[error("already removed this alarm")]
	AlreadyRemoved,
}

pub type AlarmList = Vec<(usize, Alarm)>;
pub type AlarmId = u64;
impl AlarmDatabase {
	pub fn from_db(db: &Db) -> Result<Self, sled::Error> {
		Ok(Self {
			db: db.clone(),
			storage: db.open_tree("alarms")?, //created it not exist
		})
	}

	pub async fn remove_user(&self, user_id: UserId) -> Result<(), sled::Error> {
		let mut key_begin = [std::u8::MIN; 16];
		let mut key_end = [std::u8::MAX; 16];

		BigEndian::write_u64(&mut key_begin[0..], user_id);
		BigEndian::write_u64(&mut key_end[0..], user_id);

		for key in self
			.storage
			.range(key_begin..key_end)
			.keys()
			.filter_map(Result::ok)
		{
			self.storage.remove(&key)?;
		}
		self.storage.flush_async().await?;
		Ok(())
	}

	pub fn iter(&self) -> impl Iterator<Item = (UserId, AlarmId, Alarm)> {
		self
			.storage
			.iter()
			.filter_map(Result::ok)
			.map(|(id, alarm)| {
				bincode::deserialize(&alarm).map(|alarm| {
					(
						BigEndian::read_u64(&id[0..]),
						BigEndian::read_u64(&id[8..]),
						alarm,
					)
				})
			})
			.filter_map(Result::ok)
	}

	pub fn list_users_alarms(&self, user_id: UserId) -> AlarmList {
		let mut key_begin = [std::u8::MIN; 16];
		let mut key_end = [std::u8::MAX; 16];

		BigEndian::write_u64(&mut key_begin[0..], user_id);
		BigEndian::write_u64(&mut key_end[0..], user_id);

		let alarm_list: AlarmList = self
			.storage
			.range(key_begin..key_end)
			.values()
			.filter_map(Result::ok)
			.map(|entry| bincode::deserialize::<Alarm>(&entry))
			.filter_map(Result::ok)
			.enumerate()
			.collect();
		alarm_list
	}

	pub fn remove(
		&self,
		user_id: UserId,
		counter: usize,
	) -> Result<(Alarm, AlarmId), AlarmDbError> {
		let mut key_begin = [std::u8::MIN; 16];
		let mut key_end = [std::u8::MAX; 16];

		BigEndian::write_u64(&mut key_begin[0..], user_id);
		BigEndian::write_u64(&mut key_end[0..], user_id);

		let keys: Result<Vec<sled::IVec>, sled::Error> =
			self.storage.range(key_begin..key_end).keys().collect();
		let keys = keys?;

		let key = keys.get(counter).ok_or(AlarmDbError::AlreadyRemoved)?;

		let entry = self
			.storage
			.remove(&key)?
			.ok_or(AlarmDbError::AlreadyRemoved)?;
		let alarm = bincode::deserialize::<Alarm>(&entry).unwrap();
		let alarm_id = BigEndian::read_u64(&key[8..]);
		Ok((alarm, alarm_id))
	}

	pub fn add(&self, alarm: &Alarm, user_id: UserId) -> Result<AlarmId, AlarmDbError> {
		let id = self.db.generate_id()?;
		let mut key = [0; 16];

		BigEndian::write_u64(&mut key[0..], user_id);
		BigEndian::write_u64(&mut key[8..], id);
		let data = bincode::serialize(alarm).unwrap();

		self.storage.insert(key, data)?;
		Ok(id)
	}
}
