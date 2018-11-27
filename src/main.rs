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
		let userdata = passw_db.get_userdata(username).clone();
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
			"t\n" => httpserver::signal_newdata(data_handle.clone(),0),
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
	use crate::httpserver::timeseries_interface::compression::encode;

	fn divide_ceil(x: u8, y: u8) -> u8{
		(x + y - 1) / y
	}

	#[test]
	fn check_server_security() {
		//check if putting data works
		let passw_db = Arc::new(RwLock::new(PasswordDatabase::load().unwrap()));
		let data = Arc::new(RwLock::new(timeseries_interface::init(PathBuf::from("data")).unwrap())); 
		let sessions = Arc::new(RwLock::new(HashMap::new()));
		
		let (_, web_handle) =
		httpserver::start(Path::new("keys/cert.key"), Path::new("keys/cert.cert"), data.clone(), passw_db.clone(), sessions.clone());
		let client = reqwest::Client::builder()
			.danger_accept_invalid_certs(true)
			.build()
			.unwrap();
		
		let key: u64 = 0;
		let node_id: u16 = 0;
		let temp: f32 = 20.34;
		let humidity: f32 = 53.12;

		let mut data_string: Vec<u8> = Vec::new();
		data_string.write_u16::<NativeEndian>(node_id).unwrap();
		data_string.write_u64::<NativeEndian>(key).unwrap();
		data_string.write_u16::<NativeEndian>(((temp + 20.) * 100.) as u16).unwrap();
		data_string.write_u16::<NativeEndian>((humidity * 100.) as u16).unwrap();

		println!("sending post request");
		let resp = client
			.post("https://www.deviousd.duckdns.org:8080/newdata")
			.body(data_string)
			.send()
			.unwrap();
		assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);
		
		println!("lala");
		let mut datasets = data.write().unwrap();
		let dataset = datasets.sets.get(&node_id).unwrap();
		let metadata = &dataset.metadata;
		println!("yoyo");
		let key = metadata.key;
		let mut data_string: Vec<u8> = Vec::new();
		data_string.write_u16::<NativeEndian>(node_id).unwrap();
		data_string.write_u64::<NativeEndian>(key).unwrap();
		let test_value = 10;
		println!("for loopy one");
		for _ in 0..metadata.fieldsum(){ data_string.push(0); }
		println!("hello loop");
		for field in &metadata.fields {
			println!("hello field");
			encode(test_value, &mut data_string[10..], field.offset, field.length);
		}
		println!("formatted body: {:?}", &data_string);
		let resp = client
			.post("https://www.deviousd.duckdns.org:8080/newdata")
			.body(data_string)
			.send()
			.unwrap();
		assert_eq!(resp.status(), reqwest::StatusCode::OK);
		
		////now check if the login logout sys works
		//let mut params = HashMap::new();
		//params.insert("p", "test");
		//params.insert("u", "test");
		
		//let resp = client
			//.post("https://www.deviousd.duckdns.org:8080/login/index")
			//.form(&params)
			//.send()
			//.unwrap();
		//println!("resp: {:?}",resp);
		////auth-cookie=n+ccl97c+dPXu8fn0kXBQMx230NXtoP+hkFPdi4=; HttpOnly; Secure; Path=/; Domain=deviousd.duckdns.org
		//let login_cookie = resp.headers().get("set-cookie").unwrap().to_str().unwrap().trim_end_matches("; HttpOnly; Secure; Path=/; Domain=deviousd.duckdns.org");
		
		//let resp = client
			//.post("https://www.deviousd.duckdns.org:8080/logout")
			//.header(reqwest::header::COOKIE, login_cookie)
			//.send()
			//.unwrap();
		//let logout_cookie = resp.headers().get("set-cookie").unwrap().to_str().unwrap().trim_end_matches("; HttpOnly; Secure; Path=/; Domain=deviousd.duckdns.org");
		//assert_eq!(logout_cookie, "auth-cookie=");
		
		httpserver::stop(web_handle);
	}
}
