use serde::{Serialize, Deserialize};
use log::{warn, info, debug, trace};

use byteorder::{ByteOrder, LittleEndian, NetworkEndian, WriteBytesExt};
use bytes::Bytes;
use smallvec::SmallVec;

use std::fs;
use std::fs::File;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::cmp::Ordering;

use chrono::prelude::*;

use minimal_timeseries::{Timeseries, BoundResult, DecodeParams};
use std::collections::HashMap;
use crate::httpserver::data_router_ws_client::SetSliceDecodeInfo;

pub mod specifications;
pub mod compression;
pub mod read_to_array;
pub mod read_to_packets;
pub mod data_router;
pub mod error_router;

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
	pub id: FieldId,//check if we can remove this
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

		decoded *= num::cast(self.decode_scale).unwrap();//FIXME flip decode scale / and *
		decoded += num::cast(self.decode_add).unwrap();
	
		decoded
	}
	pub fn encode<D>(&self, mut numb: T, line: &mut [u8])
	where D: num::cast::NumCast+std::fmt::Display+std::ops::Add+std::ops::SubAssign+std::ops::AddAssign+std::ops::DivAssign{

		//println!("org: {}",numb);
		numb -= num::cast(self.decode_add).unwrap();
		numb /= num::cast(self.decode_scale).unwrap();
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
		let bits = field.offset as u16 + field.length as u16;
		devide_up(bits, 8)
	}
}

pub type DatasetId = u16;
pub struct DataSet {
	pub timeseries: Timeseries, //custom file format
	pub metadata: MetaData, //is stored by serde
}

#[derive(Debug)]
pub struct ReadState {
	fields: Vec<Field<f32>>,
	start_byte: u64,
	stop_byte: u64,
	decode_params: DecodeParams,
	pub decoded_line_size: usize,
	pub numb_lines: u64,
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
			let decoded: f32 = field.decode::<f32>(&line);
			recoded_line.write_f32::<LittleEndian>(decoded).unwrap();
		}
		recoded_line.to_vec()
	}
}


#[derive(Debug, Clone, Serialize, Deserialize, Eq, Hash)]
pub enum Authorisation{
	Owner(FieldId),
	Reader(FieldId),
}

impl Ord for Authorisation{
	fn cmp(&self, other: &Self) -> Ordering {
		FieldId::from(self).cmp(&FieldId::from(other))
	}
}

impl PartialOrd for Authorisation{
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		Some(self.cmp(other))
	}
}

impl PartialEq for Authorisation {
	fn eq(&self, other: &Self) -> bool {
		FieldId::from(self) == FieldId::from(other)
	}
}

impl AsRef<FieldId> for Authorisation{
	fn as_ref(&self) -> &FieldId {
		match self{
			Authorisation::Owner(id) => id,
			Authorisation::Reader(id) => id,
		}
	}
}

impl std::convert::From<&Authorisation> for FieldId {
	fn from(auth: &Authorisation) -> FieldId {
		match auth {
			Authorisation::Owner(id) => *id,
			Authorisation::Reader(id) => *id,
		}
	}
}

pub struct Data {//TODO make multithreaded
	pub dir: PathBuf,
	free_dataset_id: u16, //replace with atomics
	pub sets: HashMap<DatasetId, DataSet>, //rwlocked hasmap + rwlocked Dataset
}

// load all the datasets and store them on theire id in a hashmap
pub fn init<P: Into<PathBuf>>(dir: P) -> Result<Data, io::Error> {
	let dir = dir.into();
	if !Path::new(&dir).exists() {
		fs::create_dir(&dir)?
	};

	let mut free_dataset_id: DatasetId = 1; //zero is reserved
	let mut sets: HashMap<DatasetId, DataSet> = HashMap::new();

	fn is_datafile(entry: &fs::DirEntry) -> bool {
		//println!("hellloooaaa: {:?}",entry.unwrap().path());
		entry
			.path()
			.to_str()
			.map(|s| s.ends_with(".dat"))
			.unwrap_or(false)
	}
	for entry in fs::read_dir(&dir).unwrap().filter_map(Result::ok) {
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
				load_data(&mut sets, &path, data_id); 
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
	pub fn add_set<T: AsRef<Path>>(&mut self, spec_path: T) -> io::Result<DatasetId>{	

		let f = fs::OpenOptions::new().read(true).write(false).create(false).open(spec_path)?;
		if let Ok(metadata) = serde_yaml::from_reader::<File, specifications::MetaDataSpec>(f) {
			let metadata: MetaData = metadata.into();
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
			info!("added timeseries under id: {}", dataset_id);
			Ok(dataset_id)
		} else {
			warn!("could not parse specification");
			Err(io::Error::new(io::ErrorKind::InvalidData, "could not parse specification"))
		}
	}

	pub fn add_specific_set(&mut self, spec: specifications::MetaDataSpec) -> io::Result<DatasetId>{
		let metadata: MetaData = spec.into();
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
	}
}

impl Data {
	pub fn authenticate_error_packet(&mut self, data_string: &Bytes) -> Result<DatasetId,()> {
		if data_string.len() < 12 {
			warn!("error_string size (={}) to small for key, datasetid and any error (min 12 bytes)", data_string.len());
			return Err(());
		}

		let dataset_id = LittleEndian::read_u16(&data_string[..2]);
		let key = LittleEndian::read_u64(&data_string[2..10]);

		if let Some(set) = self.sets.get_mut(&dataset_id){
			if key != set.metadata.key { 
				Err(()) 
			} else {
				Ok(dataset_id) 
			}
		} else {
			warn!("could not find dataset with id: {}", dataset_id);
			Err(())
		}
	}

	pub fn store_new_data(&mut self, mut data_string: Bytes, time: DateTime<Utc>) -> Result<(DatasetId, Vec<u8>), ()> {
		if data_string.len() < 11 {
			warn!("data_string size (={}) to small for key, datasetid and any data (min 11 bytes)", data_string.len());
			return Err(());
		}

		let dataset_id = LittleEndian::read_u16(&data_string[..2]);
		let key = LittleEndian::read_u64(&data_string[2..10]);

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
