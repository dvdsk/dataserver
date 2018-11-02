extern crate byteorder;
extern crate bytes;
extern crate minimal_timeseries;
extern crate walkdir;
extern crate serde_yaml;

use self::byteorder::{ByteOrder, NativeEndian};
use self::bytes::Bytes;

use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;

use self::minimal_timeseries::Timeseries;
use self::walkdir::{DirEntry, WalkDir};
use std::collections::HashMap;

mod compression;

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
struct Field {
	id: u16,
	name: String,
	
	offset: u8, //bits
	length: u8, //bits (max 32 bit variables)
	
	decode_scale: u16,
	decode_offset: u16,
}

impl Field {
	fn decode<T>(self, line: &[u8]) -> T 
	where T: From<u32>+From<u16>+std::ops::Add+std::ops::SubAssign+std::ops::DivAssign{
		let int_repr: u32 = compression::decode(line, self.offset, self.length);
		let mut decoded = T::from(int_repr);
		
		decoded -= T::from(self.decode_offset);
		decoded /= T::from(self.decode_scale);
	
		decoded
	}
}

pub struct DataSet {
	timeseries: Timeseries,
	fields: Vec<Field>,
}

// load all the datasets and store them on theire id in a hashmap
pub fn init(dir: PathBuf) -> Result<HashMap<u16, DataSet>, io::Error> {
	if !Path::new(&dir).exists() {
		fs::create_dir(&dir)?
	};

	let mut data: HashMap<u16,DataSet> = HashMap::new();

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
		load_data(&mut data, datafile.path());
	}

	Ok(data)
}

pub fn load_data(data: &mut HashMap<u16,DataSet>, datafile_path: &Path) {
	if let Ok(user_id) = datafile_path
		.file_stem()
		.unwrap()
		.to_str()
		.unwrap()
		.parse::<u16>()
	{
		let mut info_path = datafile_path.to_owned();
		info_path.set_extension(".yaml");
		if let Ok(info_file) = fs::OpenOptions::new().read(true).write(false).create(false).open(info_path) {
			if let Ok(info) = serde_yaml::from_reader::<std::fs::File, Vec<Field>>(info_file) {
				let line_size: u16 = info.iter().map(|field| field.length as u16).sum();
				if let Ok(mut timeserie) = Timeseries::open(datafile_path, line_size as usize){
					data.insert(user_id, 
						DataSet{
							timeseries: timeserie,
							fields: info,
						}
					);
				} 
			}
		}
	}
}

pub fn store_new_data(data_string: &Bytes) -> Result<(), ()> {
	let node_id = NativeEndian::read_u16(&data_string[..2]);
	//if node_id
	Ok(())
}
