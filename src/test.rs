use super::*;
use byteorder::{NativeEndian, WriteBytesExt};
use crate::httpserver::timeseries_interface::compression::encode;
use std::f32;
use chrono::{Duration, DateTime, TimeZone, NaiveDateTime, Utc};
use bytes::Bytes;

fn setup_debug_logging(verbosity: u8) -> Result<(), fern::InitError> {
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
			.level(log::LevelFilter::Info)
			.level_for("tokio_core::reactor", log::LevelFilter::Off)
			.level_for("tokio_reactor", log::LevelFilter::Off)
			.level_for("hyper", log::LevelFilter::Off)
			.level_for("reqwest", log::LevelFilter::Off),
		1 => base_config
			.level(log::LevelFilter::Debug)
			.level_for("tokio_core::reactor", log::LevelFilter::Off)
			.level_for("tokio_reactor", log::LevelFilter::Off)
			.level_for("hyper", log::LevelFilter::Off)
			.level_for("reqwest", log::LevelFilter::Off),
		2 => base_config
			.level(log::LevelFilter::Trace)
			.level_for("tokio_core::reactor", log::LevelFilter::Off)
			.level_for("tokio_reactor", log::LevelFilter::Off)
			.level_for("hyper", log::LevelFilter::Off)
			.level_for("reqwest", log::LevelFilter::Off),
		_3_or_more => base_config.level(log::LevelFilter::Trace),
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

fn divide_ceil(x: u8, y: u8) -> u8{
	(x + y - 1) / y
}

#[test]
fn check_server_security() {

	println!("test!");
	//setup_debug_logging(2).unwrap();
	//check if putting data works
	let passw_db = Arc::new(RwLock::new(PasswordDatabase::load("test").unwrap()));
	let data = Arc::new(RwLock::new(timeseries_interface::init(PathBuf::from("test/data")).unwrap()));
	let sessions = Arc::new(RwLock::new(HashMap::new()));

	println!("test!");

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

	let mut datasets = data.write().unwrap();
	let dataset = datasets.sets.get(&node_id).unwrap();
	let metadata = &dataset.metadata;
	let key = metadata.key;

	let mut data_string: Vec<u8> = Vec::new();
	data_string.write_u16::<NativeEndian>(node_id).unwrap();
	data_string.write_u64::<NativeEndian>(key).unwrap();

	let test_value = 10;
	for _ in 0..metadata.fieldsum(){ data_string.push(0); }
	for field in &metadata.fields {
		encode(test_value, &mut data_string[10..], field.offset, field.length);
	}
	std::mem::drop(datasets);//unlock rwlock
	let resp = client
		.post("https://www.deviousd.duckdns.org:8080/newdata")
		.body(data_string)
		.send()
		.unwrap();
	assert_eq!(resp.status(), reqwest::StatusCode::OK);

	httpserver::stop(web_handle);
}


fn add_template_set(passw_db: & Arc<RwLock<PasswordDatabase>>, data: & Arc<RwLock<timeseries_interface::Data>>)
	-> Result<timeseries_interface::DatasetId, ()>{
	let mut data = data.write().unwrap();
	let file_name = String::from("template.yaml");
	timeseries_interface::specifications::write_template().unwrap();
	if let Ok(dataset_id) = data.add_set(file_name){
		let username = String::from("test");

		let fields = &data.sets.get(&dataset_id).unwrap().metadata.fields;
		let mut passw_db = passw_db.write().unwrap();
		let userdata = passw_db.get_userdata(username).clone();
		passw_db.add_owner(dataset_id, fields, userdata);
		Ok(dataset_id)
	} else {
		//destroy files
		println!("could not create new template dataset");
		Err(())
	}
}

fn add_test_set(passw_db: & Arc<RwLock<PasswordDatabase>>, data: & Arc<RwLock<timeseries_interface::Data>>)
	-> Result<timeseries_interface::DatasetId, ()>{
	let mut data = data.write().unwrap();
	let file_name = String::from("test.yaml");

	if let Ok(dataset_id) = data.add_set(file_name){
		let username = String::from("test");

		let fields = &data.sets.get(&dataset_id).unwrap().metadata.fields;
		let mut passw_db = passw_db.write().unwrap();
		let userdata = passw_db.get_userdata(username).clone();
		passw_db.add_owner(dataset_id, fields, userdata);
		Ok(dataset_id)
	} else {
		//destroy files
		println!("could not create new test dataset");
		Err(())
	}
}


#[test]
#[ignore] //not run use: cargo test -- --ignored insert_timecheck_set
fn insert_timecheck_set() {
	setup_debug_logging(0).unwrap();
	//check if putting data works
	let passw_db = Arc::new(RwLock::new(PasswordDatabase::load("test").unwrap()));
	let data = Arc::new(RwLock::new(timeseries_interface::init(PathBuf::from("test/data")).unwrap()));

	let username = String::from("test");
	let password = String::from("test");

	let user_data = UserInfo{
		timeseries_with_access: HashMap::new(),
		last_login: Utc::now(),
		username: username.clone(),
	};

	let mut passw_db_unlocked = passw_db.write().unwrap();
	passw_db_unlocked.store_user(username.as_str().as_bytes(), password.as_str().as_bytes(), user_data);
	std::mem::drop(passw_db_unlocked);

	let id = add_test_set(&passw_db, &data).unwrap();

	let now = Utc::now();
	//let t_start= (now - Duration::days(1)).timestamp();

	let t_start= (now - Duration::hours(20)).timestamp();
	//first point gets the correct dataset, next is at 5/6 of the range till the end
	//then it goes back to about 1/5

	//let t_start= (now - Duration::hours(15)).timestamp();
	//let t_start= (now - Duration::hours(10)).timestamp();
	//let t_start= (now - Duration::minutes(10)).timestamp();

	let t_end = now.timestamp();
	let mut datasets = data.write().unwrap();

	let dataset = datasets.sets.get(&id).unwrap();
	let metadata = &dataset.metadata;
	let len = metadata.fieldsum();
	let fields = metadata.fields.clone();
	let key = metadata.key;
	let len = metadata.fieldsum();


	for timestamp in (t_start..t_end).step_by(5) {
		let mut data_string: Vec<u8> = Vec::new();
		data_string.write_u16::<NativeEndian>(id).unwrap();
		data_string.write_u64::<NativeEndian>(key).unwrap();
		for _ in 0..len{ data_string.push(0); }

		fields[0].encode::<u32>(timestamp as f32, &mut data_string[10..]);
		//println!("{}", field.decode::<f32>(&data_string[10..]) );

		let now = DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(timestamp, 0), Utc);
		datasets.store_new_data(Bytes::from(data_string), now);
	}

	std::mem::drop(datasets);
	//TODO send multiple lines
		//write new protocol for sending multiple lines
}


#[test]
#[ignore] //not run use: cargo test -- --ignored insert_test_set
fn insert_test_set() {
	setup_debug_logging(0).unwrap();
	//check if putting data works
	let passw_db = Arc::new(RwLock::new(PasswordDatabase::load("test").unwrap()));
	let data = Arc::new(RwLock::new(timeseries_interface::init(PathBuf::from("test/data")).unwrap()));

	let username = String::from("test");
	let password = String::from("test");

	let user_data = UserInfo{
		timeseries_with_access: HashMap::new(),
		last_login: Utc::now(),
		username: username.clone(),
	};

	let mut passw_db_unlocked = passw_db.write().unwrap();
	passw_db_unlocked.store_user(username.as_str().as_bytes(), password.as_str().as_bytes(), user_data);
	std::mem::drop(passw_db_unlocked);

	let id = add_template_set(&passw_db, &data).unwrap();

	let now = Utc::now();
	let t_start= (now - Duration::days(365)).timestamp();

	//let t_start= (now - Duration::hours(20)).timestamp();
	//first point gets the correct dataset, next is at 5/6 of the range till the end
	//then it goes back to about 1/5

	//let t_start= (now - Duration::hours(15)).timestamp();
	//let t_start= (now - Duration::hours(10)).timestamp();
	//let t_start= (now - Duration::minutes(10)).timestamp();

	let t_end = now.timestamp();
	let mut datasets = data.write().unwrap();

	let dataset = datasets.sets.get(&id).unwrap();
	let metadata = &dataset.metadata;
	let len = metadata.fieldsum();
	let fields = metadata.fields.clone();
	let key = metadata.key;
	let len = metadata.fieldsum();


	for ((timestamp, fase), i) in (t_start..t_end).step_by(5).zip((0..10_000).cycle()).zip((0..4).cycle()) {
		let angle = fase as f32 /10_000. * 2. * 3.1415;
		println!("i: {}",i);
		let mut test_value = angle.sin();//*100.+100.;
		println!("angle: {}, test_value: {}",angle,test_value);
		let mut data_string: Vec<u8> = Vec::new();
		data_string.write_u16::<NativeEndian>(id).unwrap();
		data_string.write_u64::<NativeEndian>(key).unwrap();
		for _ in 0..len{ data_string.push(0); }

		for field in &fields {
			test_value+=1.;
			field.encode::<u32>(test_value, &mut data_string[10..]);
			println!("{}", field.decode::<f32>(&data_string[10..]) );
		}
		let now = DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(timestamp, 0), Utc);
		datasets.store_new_data(Bytes::from(data_string), now);
	}

	std::mem::drop(datasets);
	//TODO send multiple lines
		//write new protocol for sending multiple lines
}


#[test]
#[ignore] //not run use: cargo test -- --ignored view_test_set
fn view_test_set() {
	//TODO print done and wait for user OK
	setup_debug_logging(0).unwrap();

	let passw_db = Arc::new(RwLock::new(PasswordDatabase::load("test").unwrap()));
	let data = Arc::new(RwLock::new(timeseries_interface::init(PathBuf::from("test/data")).unwrap()));
	let sessions = Arc::new(RwLock::new(HashMap::new()));

	let (_, web_handle) =
	httpserver::start(Path::new("keys/cert.key"), Path::new("keys/cert.cert"), data.clone(), passw_db.clone(), sessions.clone());
	let client = reqwest::Client::builder()
		.danger_accept_invalid_certs(true)
		.build()
		.unwrap();

	println!("please verify plot is correct then press enter to exit");
	let file_name: String = read!("{}\n");

	//remove_dataset(&passw_db, &data, 1);
	httpserver::stop(web_handle);
}
