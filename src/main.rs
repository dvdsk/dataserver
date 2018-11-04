#[macro_use]
extern crate log;
#[macro_use]
extern crate serde_derive;
#[macro_use] 
extern crate text_io;
extern crate chrono;

mod certificate_manager;
mod httpserver;

use self::chrono::Utc;
use self::httpserver::{timeseries_interface, secure_database::PasswordDatabase, secure_database::UserInfo};

use std::path::{Path,PathBuf};
use std::sync::{Arc, RwLock};
use std::io::{stdin, stdout, Read, Write};
use std::collections::HashMap;

pub fn pause() {
	let mut stdout = stdout();
	stdout
		.write(b"Press Enter to halt servers and quit...\n")
		.unwrap();
	stdout.flush().unwrap();
	stdin().read(&mut [0]).unwrap();
}

fn add_user(passw_db: & Arc<RwLock<PasswordDatabase>>){
	println!("enter username:");
	let username: String = read!("{}\n");
	println!("enter password:");
	let password: String = read!("{}\n");
	
	let user_data = UserInfo{
		timeseries_with_access: HashMap::new(),
		last_login: Utc::now(), 
		username: username.clone(),
	};
	
	let mut passw_db = passw_db.write().unwrap();
	passw_db.store_user(username.as_str().as_bytes(), password.as_str().as_bytes(), user_data);
}

fn add_dataset(passw_db: & Arc<RwLock<PasswordDatabase>>, data: & Arc<RwLock<timeseries_interface::Data>>){
	let mut data = data.write().unwrap();
	if let Ok(dataset_id) = data.add_set(){
		println!("enter intended owner's username:");
		let username: String = read!("{}\n");
		
		let fields = &data.sets.get(&dataset_id).unwrap().metadata.fields;		
		let mut passw_db = passw_db.write().unwrap();
		let userdata = passw_db.get_userdata(username);
		passw_db.add_owner(dataset_id, fields, userdata);
	} else {
		//destroy files
		println!("could not create new dataset");
	}
}

fn main() {
	//https://www.deviousd.duckdns.org:8080/index.html
	//only do if certs need update
	if false {
		//generate_and_sign_keys
		if let Err(error) = certificate_manager::generate_and_sign_keys(
			"deviousd.duckdns.org",
			Path::new("keys/cert.key"),
			Path::new("keys/cert.cert"),
			Path::new("keys/user.key"),
		) {
			println!("could not auto generate certificate, error: {:?}", error)
		}
	}

	let passw_db = Arc::new(RwLock::new(PasswordDatabase::load().unwrap()));
	let data = Arc::new(RwLock::new(timeseries_interface::init(PathBuf::from("data")).unwrap())); 
	let sessions = Arc::new(RwLock::new(HashMap::new()));

	let (data_handle, web_handle) =
	httpserver::start(Path::new("keys/cert.key"), Path::new("keys/cert.cert"), data.clone(), passw_db.clone(), sessions.clone());
	println!("press: t to send test data, n: to add a new user, q to quit, a to add new dataset");
	loop {
		let mut input = String::new();
		stdin().read_line(&mut input).unwrap();
		match input.as_str() {
			"t\n" => httpserver::send_newdata(data_handle.clone()),
			"n\n" => add_user(& passw_db),
			"a\n" => add_dataset(&passw_db, &data),
			"q\n" => break,
			_ => println!("unhandled"),
		};
	}

	println!("shutting down");
	httpserver::stop(web_handle);
}

#[cfg(test)]
mod tests {
	use super::*;
	extern crate byteorder;
	extern crate reqwest;
	use self::byteorder::{NativeEndian, WriteBytesExt};

	#[test]
	fn put_new_data() {
		let (data_handle, web_handle) =
			httpserver::start(Path::new("keys/cert.key"), Path::new("keys/cert.cert"));
		let client = reqwest::Client::builder()
			.danger_accept_invalid_certs(true)
			.build()
			.unwrap();

		let node_id: u16 = 2233;
		let temp: f32 = 20.34;
		let humidity: f32 = 53.12;

		let mut data_string: Vec<u8> = Vec::new();
		data_string.write_u16::<NativeEndian>(node_id).unwrap();
		data_string
			.write_u16::<NativeEndian>(((temp + 20.) * 100.) as u16)
			.unwrap();
		data_string
			.write_u16::<NativeEndian>((humidity * 100.) as u16)
			.unwrap();

		let res = client
			.post("https://www.deviousd.duckdns.org:8080/newdata")
			.body(data_string)
			.send()
			.unwrap();
		println!("res: {:?}", res);
		pause();
		httpserver::stop(web_handle);
	}
}
