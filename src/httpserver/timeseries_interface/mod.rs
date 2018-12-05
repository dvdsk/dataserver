extern crate byteorder;
extern crate bytes;
extern crate minimal_timeseries;
extern crate walkdir;
extern crate serde_yaml;
extern crate chrono;
extern crate smallvec;
extern crate num;

use self::byteorder::{ByteOrder, NativeEndian, NetworkEndian, LittleEndian, WriteBytesExt};
use self::bytes::Bytes;
use self::smallvec::SmallVec;

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
use super::websocket_client_handler::SetSliceDecodeInfo;

mod specifications;
pub mod compression;

pub type FieldId = u8;
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct Field<T> {
	id: FieldId,//check if we can remove this
	pub name: String,
	
	pub offset: u8, //bits
	pub length: u8, //bits (max 32 bit variables)
	
	pub decode_scale: T,
	pub decode_add: T,
}

//TODO do away with generics in favor for speeeeed
impl<T> Field<T>
where T: num::cast::NumCast+std::fmt::Display+std::ops::Add+std::ops::SubAssign+std::ops::DivAssign+std::marker::Copy {
	fn decode<D>(&self, line: &[u8]) -> D
	where D: num::cast::NumCast+std::fmt::Display+std::ops::Add+std::ops::SubAssign+std::ops::DivAssign+std::ops::AddAssign{
	//where D: From<T>+From<u32>+From<u16>+std::ops::Add+std::ops::SubAssign+std::ops::DivAssign+std::ops::AddAssign{
		let int_repr: u32 = compression::decode(line, self.offset, self.length);
		println!("int regr: {}", int_repr);
		let mut decoded: D = num::cast(int_repr).unwrap();
		
		println!("add: {}", self.decode_add);
		println!("scale: {}", self.decode_scale);

		decoded += num::cast(self.decode_add).unwrap();
		decoded /= num::cast(self.decode_scale).unwrap();
	
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

#[inline] fn devide_up(t: u16, n: u16) -> u16 {
	(t + (n-1))/n
}

impl MetaData {
	pub fn fieldsum(&self) -> u16 {
		let field = self.fields.last().unwrap();
		let bits = field.offset as u16 + field.length as u16;
		devide_up(bits, 8) //make this do int devide
	}
}

pub type DatasetId = u16;
pub struct DataSet {
	pub timeseries: Timeseries, //custom file format
	pub metadata: MetaData, //is stored by serde
}



impl DataSet {
	pub fn get_decode_info(&self, allowed_fields: &Vec<FieldId>) -> SetSliceDecodeInfo {
		let mut recoded_line_size = 0;
		let mut offset_in_dataset = SmallVec::<[u8; 8]>::new();
		let mut lengths = SmallVec::<[u8; 8]>::new();
		let mut offset_in_recoded = SmallVec::<[u8; 8]>::new();
		
		let mut recoded_offset = 0;
		for id in allowed_fields {
			let field = &self.metadata.fields[*id as usize];
			offset_in_dataset.push(field.offset);
			lengths.push(field.length);
			offset_in_recoded.push(recoded_offset);
			recoded_offset += field.length;
			recoded_line_size += field.length;
		}
		
		SetSliceDecodeInfo {
			field_lenghts: lengths.into_vec(),
			field_offsets: offset_in_recoded.into_vec(), 
			data_is_little_endian: cfg!(target_endian = "little"),
		}
	}
	
	pub fn get_update(&self, line: Vec<u8>, timestamp: i64, allowed_fields: &Vec<FieldId>, setid: DatasetId) 
	-> Vec<u8>{
		trace!("get_update");

		let mut recoded_line_size = 0;
		let mut offset_in_dataset = SmallVec::<[u8; 8]>::new();
		let mut lengths = SmallVec::<[u8; 8]>::new();
		let mut offset_in_recoded = SmallVec::<[u8; 8]>::new();
		
		let mut recoded_offset = 0;
		for id in allowed_fields {
			let field = &self.metadata.fields[*id as usize];
			offset_in_dataset.push(field.offset);
			lengths.push(field.length);
			offset_in_recoded.push(recoded_offset);
			recoded_offset += field.length;
			recoded_line_size += field.length;
		}
		let recoded_line_size =  (recoded_line_size as f32 /8.0).ceil() as u8; //convert to bytes
		let mut recoded_line: SmallVec<[u8; 24]> = smallvec::smallvec![0; recoded_line_size as usize + 8];
		
		recoded_line.write_u16::<NetworkEndian>(setid).unwrap();
		recoded_line.write_i64::<NetworkEndian>(timestamp).unwrap();
		for ((offset, len),recoded_offset) in offset_in_dataset.iter().zip(lengths.iter()).zip(offset_in_recoded.iter()){
			let decoded: u32 = compression::decode(&line, *offset, *len);
			compression::encode(decoded, &mut recoded_line, *recoded_offset, *len);
		}
		recoded_line.to_vec()
	}
	
	pub fn get_update_uncompressed(&self, line: Vec<u8>, timestamp: i64, allowed_fields: &Vec<FieldId>, setid: DatasetId)
	-> Vec<u8>{
		trace!("get_update_uncompressed");

		let mut recoded_line = SmallVec::<[u8; 64]>::new(); // initialize an empty vector

		//browsers tend to use little endian, thus present all data little endian
		recoded_line.write_u16::<LittleEndian>(setid).unwrap();
		recoded_line.write_f64::<LittleEndian>(timestamp as f64).unwrap();
		for field in allowed_fields.into_iter().map(|id| &self.metadata.fields[*id as usize]) {
			println!("field: {:?}",field);
			println!("line: {:?}",line);
			let decoded: f32 = field.decode::<f32>(&line);
			println!("decoded: {}", decoded);
			recoded_line.write_f32::<LittleEndian>(decoded).unwrap();
		}
		recoded_line.to_vec()
	}


	//TODO rewrite timeseries lib to allow local set bound info and passing
	//that info to the read funct
	//TODO rewrite timeseries lib to allow async access when async is introduced into rust
	pub fn get_initdata(&mut self, t_start: DateTime<Utc>, t_end: DateTime<Utc>, allowed_fields: &Vec<FieldId>)
	-> Result<(Vec<u64>, Vec<u8>, SetSliceDecodeInfo), std::io::Error> {
		//determine recoding params
		let mut recoded_line_size = 0;
		let mut offset_in_dataset = SmallVec::<[u8; 8]>::new();
		let mut lengths = SmallVec::<[u8; 8]>::new();
		let mut offset_in_recoded = SmallVec::<[u8; 8]>::new();
		
		let mut recoded_offset = 0;
		for id in allowed_fields {
			let field = &self.metadata.fields[*id as usize];
			offset_in_dataset.push(field.offset);
			lengths.push(field.length);
			offset_in_recoded.push(recoded_offset);
			recoded_offset += field.length;
			recoded_line_size += field.length;
		}
		let recoded_line_size =  (recoded_line_size as f32 /8.0).ceil() as u8; //convert to bytes
		
		//read from the dataset
		self.timeseries.set_bounds(t_start, t_end)?;
		let (timestamps, line_data) = self.timeseries.decode_sequential_time_only(100).unwrap();
		
		//shift into recoded line for transmission
		let mut recoded: Vec<u8> = Vec::with_capacity(timestamps.len()*recoded_line_size as usize);
		for line in line_data.chunks(self.timeseries.line_size) {
			let mut recoded_line: SmallVec<[u8; 16]> = smallvec::smallvec![0; recoded_line_size as usize];
			
			for ((offset, len),recoded_offset) in offset_in_dataset.iter().zip(lengths.iter()).zip(offset_in_recoded.iter()){
				let decoded: u32 = compression::decode(line, *offset, *len);
				compression::encode(decoded, &mut recoded_line, *recoded_offset, *len);
			}
			recoded.extend(recoded_line.drain());
		}
		 {
		let decode_info =  SetSliceDecodeInfo {
			field_lenghts: lengths.into_vec(),
			field_offsets: offset_in_recoded.into_vec(), 
			data_is_little_endian: cfg!(target_endian = "little"),};
		Ok((timestamps, recoded, decode_info))
		}
	}
}

//fn divide_ceil(x: u8, y: u8) -> u8{
	//(x + y - 1) / y
//}


//fn recode_line(recoded: &mut Vec<u8>, line: &[u8], pos: u8, recoded_line_size: u8, 
               //pos_in_dataset: &SmallVec::<[u8; 8]>, length: &SmallVec::<[u8; 8]>) 
               //-> SmallVec<[u8; 16]> {
	
	//let mut recoded_pos = 0;
	//let mut recoded_offset = 0;
	
	//let mut recoded_line: SmallVec<[u8; 16]> = smallvec::smallvec![0; recoded_line_size as usize];
	//let mut recoded_pos = 0;
	//for (pos, len) in pos_in_dataset.iter().zip(length.iter()){
		//let pos_bytes = divide_ceil(*pos,8);
		//let pos_offset = pos%8;
		//let len_bytes = divide_ceil(*len,8);
		
		
	//}
	//recoded_line
//}


#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub enum Authorisation{
	Owner(FieldId),
	Reader(FieldId),
}

impl AsRef<FieldId> for Authorisation{
	fn as_ref(&self) -> &FieldId {
		match self{
			Authorisation::Owner(id) => id,
			Authorisation::Reader(id) => id,
		}
	}
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
		//println!("hellloooaaa: {:?}",entry.unwrap().path());
		entry
			.path()
			.to_str()
			.map(|s| s.ends_with(".dat"))
			.unwrap_or(false)
	}
	for entry in WalkDir::new(&dir).into_iter().filter_map(Result::ok) {
		if is_datafile(&entry) {
			let path = entry.path();
			if let Ok(data_id) = path
			.file_stem()
			.unwrap()
			.to_str()
			.unwrap()
			.parse::<DatasetId>()
			{
				if data_id+1 > free_dataset_id {free_dataset_id = data_id+1; }
				load_data(&mut sets, path, data_id); 
			}
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
	info_path.set_extension("yaml");
	if let Ok(metadata_file) = fs::OpenOptions::new().read(true).write(false).create(false).open(&info_path) {
		if let Ok(metadata) = serde_yaml::from_reader::<std::fs::File, MetaData>(metadata_file) {
			let line_size: u16 = metadata.fields.iter().map(|field| field.length as u16).sum::<u16>() / 8;
			if let Ok(timeserie) = Timeseries::open(datafile_path, line_size as usize){
				info!("loaded dataset with id: {}", &data_id);
				data.insert(data_id, 
					DataSet{
						timeseries: timeserie,
						metadata: metadata,
					}
				);
			} 
		} else { warn!("could not deserialise: {:?}", info_path);}
	} else { warn!("could not open: {:?} for reading", info_path);}
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
			let f = fs::File::create(datafile_path)?;
			serde_yaml::to_writer(f, &set.metadata).unwrap();
			
			self.sets.insert(dataset_id, set);
			info!("added timeseries: {} under id: {}",name, dataset_id);
			Ok(dataset_id)
		} else {
			warn!("could not parse specification");
			Err(io::Error::new(io::ErrorKind::InvalidData, "could not parse specification"))
		}
	}
}

impl PasswordDatabase {
	pub fn add_owner(&mut self, id: DatasetId, fields: &Vec<Field<f32>>, mut userinfo: UserInfo){
		let auth_fields: Vec<Authorisation> = fields.into_iter().map(|field| Authorisation::Owner(field.id)).collect();
		userinfo.timeseries_with_access.insert(id, auth_fields);
		
		let username = userinfo.username.clone();
		self.set_userdata(username.as_str().as_bytes(), userinfo );
	}
}

impl Data {
	pub fn store_new_data(&mut self, mut data_string: Bytes, time: DateTime<Utc>) -> Result<(DatasetId, Vec<u8>), ()> {
		if data_string.len() < 11 {
			warn!("data_string size to small for key, datasetid and any data");
			return Err(());
		}
		
		let dataset_id = NativeEndian::read_u16(&data_string[..2]);
		let key = NativeEndian::read_u64(&data_string[2..10]);
		if let Some(set) = self.sets.get_mut(&dataset_id){
			if data_string.len() != set.metadata.fieldsum() as usize +10  {
				warn!("datastring has invalid length ({}) for node (id: {})", data_string.len(), dataset_id);
				return Err(());
			} else if key != set.metadata.key {
				warn!("invalid key on store new data");
				return Err(());
			}
			
			if let Err(error) = set.timeseries.append(time, &data_string[10..]){
				warn!("error on data append: {:?}",error);
				return Err(());
			}
			return Ok((dataset_id, data_string.split_off(10).to_vec() ))
		} else {
			warn!("could not find dataset");
			return Err(());
		}
	}
}

