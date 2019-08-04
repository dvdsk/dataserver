
use fern::colors::{Color, ColoredLevelConfig};
use text_io::{try_read, try_scan, read};

#[cfg(test)]
mod test;

use chrono::Utc;
use crate::httpserver::{timeseries_interface};
use crate::httpserver::secure_database::{UserDatabase, PasswordDatabase, UserInfo};

use std::path::{Path};
use std::sync::{Arc, RwLock};
use std::io::{stdin, stdout, Read, Write};
use std::collections::HashMap;

use byteorder::{NativeEndian, WriteBytesExt};
use crate::httpserver::timeseries_interface::compression::encode;
use crate::httpserver::DataRouterHandle;

pub fn pause() {
	let mut stdout = stdout();
	stdout.write_all(b"Press Enter to halt servers and quit...\n").unwrap();
	stdout.flush().unwrap();
	stdin().read_exact(&mut [0]).unwrap();
}

pub fn add_user(passw_db: &mut PasswordDatabase, user_db: &mut UserDatabase){
	println!("enter username:");
	let username: String = read!("{}\n");
	println!("enter password:");
	let password: String = read!("{}\n");

	let user_data = UserInfo{
		timeseries_with_access: HashMap::new(),
		last_login: Utc::now(),
		username: username.clone(),
	};

	passw_db.set_password(username.as_str().as_bytes(), password.as_str().as_bytes()).unwrap();
	user_db.set_userdata(user_data).unwrap();
}

pub fn add_fields_to_user(user_db: &mut UserDatabase){
	use timeseries_interface::{DatasetId, FieldId};

	println!("enter username:");
	let username: String = read!("{}\n");

	println!("enter dataset id:");
	let dataset_id: DatasetId = read!("{}\n");

	println!("enter space seperated list of field ids:");
	let fields: String = read!("{}\n");//TODO parse to fields vector
	let fields: Result<Vec<_>, _> = fields.split_whitespace().map(|x| x.parse::<FieldId>() ).collect();
	match fields {
		Ok(fields) => {
			let userdata = user_db.get_userdata(username).unwrap();
			user_db.add_owner_from_field_id(dataset_id, &fields, userdata).unwrap();
		}
		Err(_) => {
			println!("error parsing fields");
		}
	}
}

pub fn add_dataset(user_db: &mut UserDatabase, data: &Arc<RwLock<timeseries_interface::Data>>){

	if !Path::new("specs/template.yaml").exists() {
		timeseries_interface::specifications::write_template().unwrap();
	}
	if !Path::new("specs/template_for_test.yaml").exists() {
		timeseries_interface::specifications::write_template_for_test().unwrap();
	}

	println!("enter the name of the info file in the specs subfolder:");
	let file_name: String = read!("{}\n");

	let mut data = data.write().unwrap();
	if let Ok(dataset_id) = data.add_set(file_name){
		println!("enter intended owner's username:");
		let username: String = read!("{}\n");

		let fields = &data.sets.get(&dataset_id).unwrap().metadata.fields;
		let userdata = user_db.get_userdata(username).unwrap();
		user_db.add_owner(dataset_id, fields, userdata).unwrap();
	} else {
		//destroy files
		println!("could not create new dataset");
	}
}

pub fn remove_dataset(user_db: &mut UserDatabase, data: & Arc<RwLock<timeseries_interface::Data>>, id: timeseries_interface::DatasetId){
	let mut data = data.write().unwrap();
	if data.remove_set(id).is_ok(){
		let usernames: Vec<Vec<u8>> = user_db.storage.keys(&[0]).filter_map(Result::ok).collect();
		for username in usernames { //TODO finish
			let mut user_data = user_db.get_userdata(username).unwrap();
			user_data.timeseries_with_access.remove(&id);
			user_db.set_userdata(user_data).unwrap();
		}
	} else {
		//destroy files
		println!("could not remove dataset");
	}
}

/// Note the test value will be shifted on decoding.
pub fn send_test_data_over_http(data: Arc<RwLock<timeseries_interface::Data>>, port: u16){
	let node_id = 0;
	let client = reqwest::Client::builder()
	.danger_accept_invalid_certs(true)
	.build()
	.unwrap();

	let datasets = data.write().unwrap();
	let dataset = datasets.sets.get(&node_id).unwrap();
	let metadata = &dataset.metadata;
	let key = metadata.key;

	let mut data_string: Vec<u8> = Vec::new();
	data_string.write_u16::<NativeEndian>(node_id).unwrap();
	data_string.write_u64::<NativeEndian>(key).unwrap();

	let test_value = 120;
	for _ in 0..metadata.fieldsum(){ data_string.push(0); }
	for field in &metadata.fields {
		println!("offset: {}, length: {}, {}, {}",field.offset, field.length, field.decode_scale, field.decode_add);
		encode(test_value, &mut data_string[10..], field.offset, field.length);
		dbg!(field.offset);
		dbg!(field.length);
		use crate::httpserver::timeseries_interface::compression::decode;
		println!("decoded: {}", decode(&data_string[10..], field.offset, field.length));
	}
	println!("datastring: {:?}",data_string);
	std::mem::drop(datasets);//unlock rwlock
	let _ = client
		.post(&format!("{}{}{}","https://www.deviousd.duckdns.org:",port,"/newdata"))
		.body(data_string)
		.send()
		.unwrap();
}

pub fn signal_and_append_test_data(dataset_handle: Arc<RwLock<timeseries_interface::Data>>,
                                   data_router_handle: &DataRouterHandle){
	const LOCAL_SENSING_ID: u16 = 0;
	//load the requird dataset
	let data = dataset_handle.read().unwrap();
	let dataset = data.sets.get(&LOCAL_SENSING_ID).expect("dataset for local sensors is missing");
	let fields = dataset.metadata.fields.clone();
	drop(data);

	let mut line: Vec<u8> = vec!(0;64);
	//get all measurements
	let (humidity, _temperature, _pressure) = (10.,20.,30.);
	let now = Utc::now();

	//encode all data
	//dbg!(fields);
	fields[0].encode::<f32>(humidity, &mut line);
	//fields[1].encode::<f32>(temperature, &mut line);
	//fields[2].encode::<f32>(pressure, &mut line);

	//store data and send to active web_clients
	crate::httpserver::signal_newdata(&data_router_handle, LOCAL_SENSING_ID, line.clone(), now.timestamp() );
	let mut data = dataset_handle.write().unwrap();
	let set = data.sets.get_mut(&LOCAL_SENSING_ID).unwrap();
	set.timeseries.append(now, &line).unwrap();
}


pub fn setup_logging(verbosity: u8) -> Result<(), fern::InitError> {
	let mut base_config = fern::Dispatch::new();
	let colors = ColoredLevelConfig::new()
	             .info(Color::Green)
	             .debug(Color::Yellow)
	             .warn(Color::Magenta);

	base_config = match verbosity {
		0 =>
			// Let's say we depend on something which whose "info" level messages are too
			// verbose to include in end-user output. If we don't need them,
			// let's not include them.
			base_config
					.level(log::LevelFilter::Warn)
					.level_for("dataserver", log::LevelFilter::Trace)
					.level_for("minimal_timeseries", log::LevelFilter::Trace),
		1 =>
			// Let's say we depend on something which whose "info" level messages are too
			// verbose to include in end-user output. If we don't need them,
			// let's not include them.
			base_config
					.level(log::LevelFilter::Warn)
					.level_for("dataserver", log::LevelFilter::Info)
					.level_for("minimal_timeseries", log::LevelFilter::Info),
		2 =>
			// Let's say we depend on something which whose "info" level messages are too
			// verbose to include in end-user output. If we don't need them,
			// let's not include them.
			base_config.level(log::LevelFilter::Info)
					.level_for("actix-web", log::LevelFilter::Warn)
					.level_for("dataserver", log::LevelFilter::Trace)
					.level_for("minimal_timeseries", log::LevelFilter::Info),
		3 =>
			// Let's say we depend on something which whose "info" level messages are too
			// verbose to include in end-user output. If we don't need them,
			// let's not include them.
			base_config.level(log::LevelFilter::Trace),
		4 =>
			// Let's say we depend on something which whose "info" level messages are too
			// verbose to include in end-user output. If we don't need them,
			// let's not include them.
			base_config.level(log::LevelFilter::Error),
		_3_or_more => base_config.level(log::LevelFilter::Warn),
	};

	// Separate file config so we can include year, month and day in file logs
	let file_config = fern::Dispatch::new()
		.format(|out, message, record| {
			out.finish(format_args!(
				"{}[{}][{}] {}",
				chrono::Local::now().format("[%Y-%m-%d][%H:%M:%S]"),
				record.target(),
				record.level(),
				message
			))
		})
		.chain(fern::log_file("program.log")?);

	let stdout_config = fern::Dispatch::new()
		.format(move |out, message, record| {
				out.finish(format_args!(
						"[{}][{}][{}] {}",
					chrono::Local::now().format("%H:%M"),
					record.target(),
					colors.color(record.level()),
					message
				))
		})
		.chain(std::io::stdout());

	base_config.chain(file_config).chain(stdout_config).apply()?;
	Ok(())
}
