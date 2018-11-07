extern crate byteorder;
extern crate bytes;
extern crate minimal_timeseries;
extern crate walkdir;
extern crate serde_yaml;
extern crate chrono;

use self::byteorder::{ByteOrder, NativeEndian};
use self::bytes::Bytes;

use std::fs;
use std::fs::File;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use chrono::prelude::*;

use self::minimal_timeseries::Timeseries;
use self::walkdir::{DirEntry, WalkDir};
use std::collections::HashMap;

use super::secure_database::{PasswordDatabase, UserInfo};

mod specifications;
mod compression;

type FieldId = u8;
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct Field<T> {
	id: FieldId,//check if we can remove this
	pub name: String,
	
	offset: u8, //bits
	length: u8, //bits (max 32 bit variables)
	
	decode_scale: T,
	decode_add: T,
}

impl<T> Field<T> 
where T: std::ops::Add+std::ops::SubAssign+std::ops::DivAssign {
	fn decode<D>(self, line: &[u8]) -> D 
	where D: From<T>+From<u32>+From<u16>+std::ops::Add+std::ops::SubAssign+std::ops::DivAssign+std::ops::AddAssign{
		let int_repr: u32 = compression::decode(line, self.offset, self.length);
		let mut decoded = D::from(int_repr);
		
		decoded += D::from(self.decode_add);
		decoded /= D::from(self.decode_scale);
	
		decoded
	}
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct MetaData {
	pub name: String,
	pub description: String,
	pub key: u64,
	pub fields: Vec<Field<f32>>,//must be sorted lowest id to highest
}

impl MetaData {
	pub fn fieldsum(&self) -> u16 {
		let field = self.fields.last().unwrap();
		field.offset as u16 + field.length as u16
	}
}

pub type DatasetId = u16;
pub struct DataSet {
	timeseries: Timeseries, //custom file format
	pub metadata: MetaData, //is stored by serde
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub enum Authorisation{
	Owner(FieldId),
	Reader(FieldId),
}

pub struct Data {
	dir: PathBuf,
	free_dataset_id: u16,
	pub sets: HashMap<DatasetId, DataSet>,
}

// load all the datasets and store them on theire id in a hashmap
pub fn init(dir: PathBuf) -> Result<Data, io::Error> {
	if !Path::new(&dir).exists() {
		fs::create_dir(&dir)?
	};

	let mut free_dataset_id: DatasetId = 0;
	let mut sets: HashMap<DatasetId, DataSet> = HashMap::new();

	fn is_datafile(entry: &DirEntry) -> bool {
		entry
			.file_name()
			.to_str()
			.map(|s| s.ends_with(".data"))
			.unwrap_or(false)
	}

	for datafile in WalkDir::new(&dir)
		.into_iter()
		.filter_entry(|e| is_datafile(e))
		.filter_map(Result::ok)
	{
		let path = datafile.path();
		if let Ok(data_id) = path
		.file_stem()
		.unwrap()
		.to_str()
		.unwrap()
		.parse::<DatasetId>()
		{
			if data_id> free_dataset_id {free_dataset_id = data_id; }
			load_data(&mut sets, path, data_id); 
		}
	}

	Ok(Data{
		dir: dir,
		free_dataset_id: free_dataset_id,
		sets: sets,
	})
}

pub fn load_data(data: &mut HashMap<DatasetId,DataSet>, datafile_path: &Path, data_id: DatasetId) {
	let mut info_path = datafile_path.to_owned();
	info_path.set_extension(".yaml");
	if let Ok(metadata_file) = fs::OpenOptions::new().read(true).write(false).create(false).open(info_path) {
		if let Ok(metadata) = serde_yaml::from_reader::<std::fs::File, MetaData>(metadata_file) {
			let line_size: u16 = metadata.fields.iter().map(|field| field.length as u16).sum();
			if let Ok(mut timeserie) = Timeseries::open(datafile_path, line_size as usize){
				data.insert(data_id, 
					DataSet{
						timeseries: timeserie,
						metadata: metadata,
					}
				);
			} 
		}
	}
}

impl Data {
	pub fn add_set(&mut self) -> io::Result<DatasetId>{
		//create template file if it does not exist
		if !Path::new("specs/template.yaml").exists() {
			specifications::write_template()?;
		}
		println!("enter the name of the info file in the specs subfolder:");
		let file_name: String = read!("{}\n");
		let mut metadata_path = PathBuf::from("specs");
		metadata_path.push(file_name);
		metadata_path.set_extension("yaml");
		
		let f = fs::OpenOptions::new().read(true).write(false).create(false).open(metadata_path)?;
		if let Ok(metadata) = serde_yaml::from_reader::<File, specifications::MetaDataSpec>(f) {
			let metadata: MetaData = metadata.into();
			let name = metadata.name.clone();
			let line_size: u16 = metadata.fieldsum();
			let dataset_id = self.free_dataset_id;
			self.free_dataset_id += 1;
			let mut datafile_path = self.dir.clone();
			datafile_path.push(dataset_id.to_string());
			
			let set = DataSet {
				timeseries: Timeseries::open(&datafile_path, line_size as usize)?,
				metadata: metadata,
			};
			datafile_path.set_extension("yaml");
			let mut f = fs::File::create(datafile_path)?;
			serde_yaml::to_writer(f, &set.metadata).unwrap();
			
			self.sets.insert(dataset_id, set);
			println!("added timeseries: {} under id: {}",name, dataset_id);
			Ok(dataset_id)
		} else {
			println!("could not parse specification");
			Err(io::Error::new(io::ErrorKind::InvalidData, "could not parse specification"))
		}
	}
}

impl PasswordDatabase {
	pub fn add_owner(&mut self, id: DatasetId, fields: &Vec<Field<f32>>, mut userinfo: UserInfo){
		let auth_fields: Vec<Authorisation> = fields.into_iter().map(|field| Authorisation::Owner(field.id)).collect();
		userinfo.timeseries_with_access.insert(id, auth_fields);
		
		let username = userinfo.username.as_str().as_bytes();
		self.set_userdata(username, userinfo.clone() );
	}
}

impl Data {
	pub fn store_new_data(&mut self, data_string: &Bytes, time: DateTime<Utc>) -> Result<(), ()> {
		let node_id = NativeEndian::read_u16(&data_string[..2]);
		let key = NativeEndian::read_u64(&data_string[2..10]);
		if let Some(set) = self.sets.get_mut(&node_id){
			if set.metadata.key == key {
				set.timeseries.append(time, &data_string[10..]);
				return Ok(()) 
			}
		} else {
			println!("could not find dataset");
		}
		
		Err(())
	}
}
