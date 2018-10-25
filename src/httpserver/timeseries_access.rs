extern crate byteorder;
extern crate bytes;
extern crate minimal_timeseries;
extern crate walkdir;

use self::byteorder::{ByteOrder, NativeEndian};
use self::bytes::Bytes;

use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;

use self::minimal_timeseries::Timeseries;
use self::walkdir::{DirEntry, WalkDir};
use std::collections::HashMap;

pub fn init(dir: PathBuf) -> Result<HashMap<u16, Timeseries>, io::Error> {
	if !Path::new(&dir).exists() {
		fs::create_dir(&dir)?
	};

	let mut data = HashMap::new();

	fn is_datafile(entry: &DirEntry) -> bool {
		entry
			.file_name()
			.to_str()
			.map(|s| s.ends_with(".data"))
			.unwrap_or(false)
	}

	for datafile in WalkDir::new(dir)
		.into_iter()
		.filter_entry(|e| is_datafile(e))
		.filter_map(Result::ok)
	{
		load_user(&mut data, datafile.path());
	}

	Ok(data)
}

pub fn load_user(subscriber_data: &mut HashMap<u16, Timeseries>, datafile_path: &Path) {
	if let Ok(userId) = datafile_path
		.file_stem()
		.unwrap()
		.to_str()
		.unwrap()
		.parse::<u16>()
	{
		let mut sensor_data = Timeseries::open(datafile_path, test_user::LINE_SIZE).unwrap();
		subscriber_data.insert(userId, sensor_data);
	}
}

pub fn store_new_data(data_string: &Bytes) -> Result<(), ()> {
	let node_id = NativeEndian::read_u16(&data_string[..2]);
	//if node_id
	Ok(())
}

mod test_user {
	use super::*;

	pub const LINE_SIZE: usize = 10;

	pub fn decode_1(data_string: &Bytes) {
		let temp = NativeEndian::read_u16(&data_string[2..4]);
		let humidity = NativeEndian::read_u16(&data_string[4..6]);
	}
}
//let node_id: u16 = 2233;
//let temp: f32 = 20.34;
//let humidity: f32 = 53.12;

//let mut data_string: Vec<u8> = Vec::new();
//data_string.write_u16::<NativeEndian>(node_id).unwrap();
//data_string.write_u16::<NativeEndian>(((temp+20.)*100.) as u16).unwrap();
//data_string.write_u16::<NativeEndian>((humidity*100.) as u16).unwrap();
