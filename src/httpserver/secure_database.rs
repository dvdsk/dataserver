extern crate ring;
extern crate chrono;
extern crate bincode;

use httpserver::timeseries_interface;

use self::bincode::{deserialize_from,serialize_into};

use self::ring::{digest, pbkdf2};
use std::collections::HashMap;
use std::fs::{OpenOptions,File};
use std::io::Error as ioError;
use std::path::PathBuf;
use self::chrono::{DateTime, Utc};

static DIGEST_ALG: &'static digest::Algorithm = &digest::SHA256;
const CREDENTIAL_LEN: usize = digest::SHA256_OUTPUT_LEN;
pub type Credential = [u8; CREDENTIAL_LEN];

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct User {
	password: Credential,
	user_data: UserInfo,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct UserInfo {
	pub timeseries_with_access: HashMap<timeseries_interface::DatasetId, Vec<timeseries_interface::Authorisation>>,
	pub last_login: DateTime<Utc>, 
	pub username: String,
}

pub enum Error {
	WrongUsername,
	WrongPassword,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct PasswordDatabase {
    pub pbkdf2_iterations: u32,
    pub db_salt_component: [u8; 16],

    // Normally this would be a persistent database.
    // use some embedded database
    pub storage: HashMap<Vec<u8>, User>,
}

impl PasswordDatabase {
	pub fn load() -> Result<Self,ioError> {
		let database_path = PathBuf::from("userdb.hm");
		
		if database_path.exists() {
			if let Ok(f) = OpenOptions::new().read(true).write(true).open(&database_path) {
				if let Ok(database) = deserialize_from::<File, PasswordDatabase>(f) {
					println!("{:?}", database);
					Ok(database)
				} else { warn!("could not parse: {:?}", database_path); Self::new() }
			} else { warn!("could not open file: {:?}", database_path); Self::new() }
		} else { warn!("could not find the path: {:?}", database_path); Self::new() }
	}
	
	pub fn write(&mut self) {
		let database_path = PathBuf::from("userdb.hm");
		let f = OpenOptions::new().write(true).create(false).
									             truncate(true).open(&database_path).unwrap();
		serialize_into::<File, PasswordDatabase>(f, self).unwrap();		
	}
	
	pub fn new() -> Result<Self,ioError> {
		let database_path = PathBuf::from("userdb.hm");
		
		let database = PasswordDatabase {
			pbkdf2_iterations: 100_000,
			db_salt_component: [
				// This value was generated from a secure PRNG. //TODO check this
				0xd6, 0x26, 0x98, 0xda, 0xf4, 0xdc, 0x50, 0x52,
				0x24, 0xf2, 0x27, 0xd1, 0xfe, 0x39, 0x01, 0x8a
			],
				storage: HashMap::new(),
		};
		let f = OpenOptions::new().write(true).create(true).
									             truncate(true).open(&database_path)?;

		serialize_into::<File, PasswordDatabase>(f, &database).unwrap();
		warn!("created new database: {:?}", database_path);
		Ok(database)
	}
	
	pub fn store_user(&mut self, username: &[u8], password: &[u8], user_data: UserInfo) {
		let salt = self.salt(username);
		let mut to_store: Credential = [0u8; CREDENTIAL_LEN];
		pbkdf2::derive(DIGEST_ALG, self.pbkdf2_iterations, &salt,
									 password, &mut to_store);
		
		let new_user = User {password: to_store, user_data: user_data};
		self.storage.insert(Vec::from(username), new_user);
		self.write();
	}

	pub fn verify_password(&self, username: &[u8], attempted_password: &[u8])
	-> Result<(), Error> {
		match self.storage.get(username) {
		 Some(user) => {
			 let actual_password: Credential = user.password;
			 let salt = self.salt(username);
			 pbkdf2::verify(DIGEST_ALG, self.pbkdf2_iterations, &salt,
											attempted_password,
											&actual_password)
										 .map_err(|_| Error::WrongPassword)
		 },
		 None => Err(Error::WrongUsername)
		}
	}

	pub fn is_user_in_database(&self, username: &[u8]) -> bool {
		match self.storage.get(username) {
			Some(_) => true,
			None => false,
		}
	}

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

impl PasswordDatabase {
	pub fn get_userdata<T: AsRef<str>>(&mut self, username: T) -> &mut UserInfo {
		let username = username.as_ref().as_bytes();
		match self.storage.get(username) {
			Some(user) => &mut user.user_data,
			None => panic!("User database corrupt!"),
		}
	}
	
	#[allow(dead_code)]
	pub fn set_userdata(&mut self, username: &[u8], user_data: UserInfo) {
		match self.storage.remove(username) {
			Some(mut user) => {
				user.user_data = user_data;
				self.storage.insert(Vec::from(username), user);
				self.write();
			},
			None => panic!("user not found in database!"),
		}
	}
	
	pub fn update_last_login(&mut self, username: &[u8]) {
		match self.storage.remove(username) {
			Some(mut user) => {
				user.user_data.last_login = Utc::now();
				self.storage.insert(Vec::from(username), user);
				self.write();
			},
			None => panic!("user not found in database!"),
		}
	}
}
		
//fn main() {
//    // Normally these parameters would be loaded from a configuration file.
//    let mut db = PasswordDatabase {
//        pbkdf2_iterations: 100_000,
//        db_salt_component: [
//            // This value was generated from a secure PRNG.
//            0xd6, 0x26, 0x98, 0xda, 0xf4, 0xdc, 0x50, 0x52,
//            0x24, 0xf2, 0x27, 0xd1, 0xfe, 0x39, 0x01, 0x8a
//        ],
//        storage: HashMap::new(),
//    };

//    db.store_password("alice", "@74d7]404j|W}6u");

//    // An attempt to log in with the wrong password fails.
//    assert!(db.verify_password("alice", "wrong password").is_err());

//    // Normally there should be an expoentially-increasing delay between
//    // attempts to further protect against online attacks.

//    // An attempt to log in with the right password succeeds.
//    assert!(db.verify_password("alice", "@74d7]404j|W}6u").is_ok());
//}
