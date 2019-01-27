extern crate byteorder;
extern crate bytes;
extern crate minimal_timeseries;
extern crate walkdir;
extern crate serde_yaml;
extern crate chrono;
extern crate smallvec;
extern crate num;

use self::byteorder::{ByteOrder, LittleEndian, NativeEndian, NetworkEndian, WriteBytesExt};
use self::bytes::Bytes;
use self::smallvec::SmallVec;

use std::fs;
use std::fs::File;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use chrono::prelude::*;

use self::minimal_timeseries::{Timeseries, BoundResult, DecodeParams};
use self::walkdir::{DirEntry, WalkDir};
use std::collections::HashMap;

use super::secure_database::{PasswordDatabase, UserInfo};
use super::websocket_client_handler::SetSliceDecodeInfo;

pub mod specifications;
pub mod compression;

use std::f64;
trait FloatIterExt {
	  fn float_min(&mut self) -> f64;
	  fn float_max(&mut self) -> f64;
}

impl<T> FloatIterExt for T where T: Iterator<Item=f64> {
	  fn float_max(&mut self) -> f64 {
	      self.fold(f64::NAN, f64::max)
	  }
	  fn float_min(&mut self) -> f64 {
	     self.fold(f64::NAN, f64::min)
	  }
}

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
where T: num::cast::NumCast+std::fmt::Display+std::ops::Add+std::ops::SubAssign+std::ops::DivAssign+std::ops::MulAssign+std::marker::Copy {
	pub fn decode<D>(&self, line: &[u8]) -> D
	where D: num::cast::NumCast+std::fmt::Display+std::ops::Add+std::ops::SubAssign+std::ops::MulAssign+std::ops::AddAssign{
	//where D: From<T>+From<u32>+From<u16>+std::ops::Add+std::ops::SubAssign+std::ops::DivAssign+std::ops::AddAssign{
		let int_repr: u32 = compression::decode(line, self.offset, self.length);
		//println!("int regr: {}", int_repr);
		let mut decoded: D = num::cast(int_repr).unwrap();
		
		//println!("add: {}", self.decode_add);
		//println!("scale: {}", self.decode_scale);

		decoded += num::cast(self.decode_add).unwrap();
		decoded *= num::cast(self.decode_scale).unwrap();//FIXME flip decode scale / and *
	
		decoded
	}
	pub fn encode<D>(&self, mut numb: T, line: &mut [u8])
	where D: num::cast::NumCast+std::fmt::Display+std::ops::Add+std::ops::SubAssign+std::ops::AddAssign+std::ops::DivAssign{

		//println!("org: {}",numb);
		numb /= num::cast(self.decode_scale).unwrap();
		numb -= num::cast(self.decode_add).unwrap();
		//println!("scale: {}, add: {}, numb: {}", self.decode_scale, self.decode_add, numb);

		let to_encode: u32 = num::cast(numb).unwrap();

		compression::encode(to_encode, line, self.offset, self.length);
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
		info!("fname: {}",field.name);
		let bits = field.offset as u16 + field.length as u16;
		devide_up(bits, 8) //make this do int devide
	}
}

pub type DatasetId = u16;
pub struct DataSet {
	pub timeseries: Timeseries, //custom file format
	pub metadata: MetaData, //is stored by serde
}

#[derive(Debug)]
pub struct ReadState {
	pub timestamps_u64: Vec<u64>,
	pub line_data: Vec<u8>,

	fields: Vec<Field<f32>>,
	start_byte: u64,
	stop_byte: u64,
	decode_params: DecodeParams,
	pub decoded_line_size: usize,
	pub numb_lines: usize,
}

impl ReadState {
	pub fn bytes_to_read(&self) -> u64{
		self.stop_byte - self.start_byte
	}
}

impl DataSet {
	pub fn get_decode_info(&self, allowed_fields: &Vec<FieldId>) -> SetSliceDecodeInfo {
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
		debug!("setid: {}", setid);
		recoded_line.write_u16::<LittleEndian>(setid).unwrap();
		recoded_line.write_f64::<LittleEndian>(timestamp as f64).unwrap();
		for field in allowed_fields.into_iter().map(|id| &self.metadata.fields[*id as usize]) {
			//println!("field: {:?}",field);
			//println!("line: {:?}",line);
			let decoded: f32 = field.decode::<f32>(&line);
			//println!("decoded: {}", decoded);
			recoded_line.write_f32::<LittleEndian>(decoded).unwrap();
		}
		recoded_line.to_vec()
	}

	pub fn prepare_read(&mut self, t_start: DateTime<Utc>, t_end: DateTime<Utc>, requested_fields: &Vec<FieldId>)
	-> Option<ReadState>{
		//determine recoding params

		match self.timeseries.get_bounds(t_start, t_end){
			BoundResult::IoError(error) => {
				warn!("could not read timeseries, io error: {:?}", error);
				return None;
			},
			BoundResult::NoData => {
				warn!("no data within the given time points");
				return None;
			},
			BoundResult::Ok((start_byte, stop_byte, decode_params)) => {
				let fields: Vec<Field<f32>> = requested_fields.into_iter()
				.map(|id| self.metadata.fields[*id as usize].clone() ).collect();

				let numb_lines = (stop_byte as usize-start_byte as usize)/self.timeseries.full_line_size;
				let decoded_line_size = fields.len()*std::mem::size_of::<f32>();

				Some( ReadState {
					timestamps_u64: Vec::with_capacity(numb_lines),
					line_data: Vec::with_capacity(numb_lines*self.timeseries.line_size),
					fields,
					start_byte,
					stop_byte,
					decode_params,
					decoded_line_size,
					numb_lines
				})
			},
		}
	}

	pub fn get_data_chunk_uncompressed(&mut self, state: &mut ReadState, chunk_size: usize, package_numb: u16, dataset_id: u16)
	-> Option<Vec<u8>> {

		//TODO refactor to: timestamps, line_data = self.timeseries.decode_time_into_given
		let chunk_size_in_lines = chunk_size/(state.decoded_line_size+std::mem::size_of::<f64>());
		if self.timeseries.decode_time_into_given(
			&mut state.timestamps_u64,
			&mut state.line_data,
			chunk_size_in_lines,
			&mut state.start_byte,
			state.stop_byte,
			&mut state.decode_params).is_ok() {

			dbg!(chunk_size_in_lines);
			dbg!(state.decoded_line_size);
			dbg!(state.line_data.len());

			let mut buffer = Vec::with_capacity(chunk_size);
			dbg!(buffer.len());

			//write packet info
			buffer.write_u16::<LittleEndian>(package_numb).unwrap();
			buffer.write_u16::<LittleEndian>(dataset_id).unwrap();
			//add padding
			buffer.write_u32::<LittleEndian>(0).unwrap();
			for ts in &state.timestamps_u64 {
				buffer.write_f64::<LittleEndian>(*ts as f64).unwrap();
			}

			for line in state.line_data.chunks(self.timeseries.line_size) {
				for field in &state.fields {
					let decoded: f32 = field.decode::<f32>(&line);
					buffer.write_f32::<LittleEndian>(decoded).unwrap();
				}
			}
			dbg!(state.timestamps_u64.len());
			dbg!(state.line_data.len());
			dbg!(buffer.len());
			Some(buffer)
		} else {
			None
		}
	}
}


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
			//let line_size: u16 = metadata.fields.iter().map(|field| field.length as u16).sum::<u16>() / 8;

			let line_size = metadata.fieldsum();

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
	pub fn add_set(&mut self, file_name: String) -> io::Result<DatasetId>{
		//create template file if it does not exist
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
			let f = fs::File::create(datafile_path).unwrap();
			serde_yaml::to_writer(f, &set.metadata).unwrap();
			
			self.sets.insert(dataset_id, set);
			info!("added timeseries: {} under id: {}",name, dataset_id);
			Ok(dataset_id)
		} else {
			warn!("could not parse specification");
			Err(io::Error::new(io::ErrorKind::InvalidData, "could not parse specification"))
		}
	}
	pub fn remove_set(&mut self, id: DatasetId) -> io::Result<()>{

		if self.sets.remove(&id).is_none(){
			warn!("set with id: {}, can not be removed as does not exist",id);
			return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "dataset does not exist!"));
		}

		let mut datafile_path = self.dir.clone();
		datafile_path.push(id.to_string());

		datafile_path.set_extension("yaml");
		fs::remove_file(&datafile_path)?;

		datafile_path.set_extension("h");
		fs::remove_file(&datafile_path)?;

		datafile_path.set_extension("dat");
		fs::remove_file(&datafile_path)?;

		Ok(())
	}
}

impl PasswordDatabase {
	pub fn add_owner(&mut self, id: DatasetId, fields: &Vec<Field<f32>>, mut userinfo: UserInfo){
		let auth_fields: Vec<Authorisation> = fields.into_iter().map(|field| Authorisation::Owner(field.id)).collect();
		userinfo.timeseries_with_access.insert(id, auth_fields);
		
		let username = userinfo.username.clone();
		self.set_userdata(username.as_str().as_bytes(), userinfo );
	}
	// pub fn remove_owner(&mut self, id: DatasetId, &mut userinfo: UserInfo){
	// 	userinfo.timeseries_with_access.remove(&id);

	// 	let username = userinfo.username.clone();
	// 	self.set_userdata(username.as_str().as_bytes(), userinfo );
	// }
}

impl Data {
	pub fn store_new_data(&mut self, mut data_string: Bytes, time: DateTime<Utc>) -> Result<(DatasetId, Vec<u8>), ()> {
		if data_string.len() < 11 {
			warn!("data_string size to small for key, datasetid and any data");
			return Err(());
		}
		
		let dataset_id = NativeEndian::read_u16(&data_string[..2]);
		debug!("datasetid array: {:?}", &data_string[..2]);
		let key = NativeEndian::read_u64(&data_string[2..10]);
		if let Some(set) = self.sets.get_mut(&dataset_id){
			if data_string.len() != set.metadata.fieldsum() as usize +10  {
				warn!("datastring has invalid length ({}) for node (id: {}), should have length: {}", data_string.len(), dataset_id, set.metadata.fieldsum()+10);
				return Err(());
			}
			if key != set.metadata.key {
				warn!("invalid key: {}, on store new data", key);
				return Err(());
			}
			const PRINTVALUES: bool = false; //for debugging
			if PRINTVALUES {
				let mut list = String::from("");
				for field in &set.metadata.fields {
					//println!("field: {:?}",field);
					//println!("line: {:?}",line);
					let decoded: f32 = field.decode::<f32>(&data_string[10..]);
					list.push_str(&format!("{}: {}\n", field.name, decoded));

				}
				println!("{}", list);
			}

			if let Err(error) = set.timeseries.append(time, &data_string[10..]){
			//if let Err(error) = set.timeseries.append_fast(time, &data_string[10..]){
				warn!("error on data append: {:?}",error);
				return Err(());
			}
			return Ok((dataset_id, data_string.split_off(10).to_vec() ))
		} else {
			warn!("could not find dataset with id: {}", dataset_id);
			return Err(());
		}
	}
}

