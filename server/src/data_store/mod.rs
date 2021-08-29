use log::{debug, info, trace, warn};
use serde::{Deserialize, Serialize};

use byteorder::{ByteOrder, LittleEndian, WriteBytesExt};
use actix_web::web::Bytes;
use smallvec::SmallVec;

use std::cmp::Ordering;
use std::fs;
use std::fs::File;
use std::io;
use std::path::Path;
use std::path::PathBuf;

use chrono::prelude::*;

use crate::httpserver::data_router_ws_client::SetSliceDecodeInfo;
use bitspec::{Field, FieldId, MetaDataSpec, Meta, FixedLine};
use byteseries::{self, Decoder, Series};
use std::collections::HashMap;

//pub mod specifications;
pub mod data_router;
pub mod error_router;

use std::f64;

pub type DatasetId = u16;
pub struct DataSet {
	pub timeseries: Series, //custom file format
	pub metadata: FixedLine, //is stored by serde
}

#[derive(Debug, Clone)]
pub struct FieldDecoder {
	fields: Vec<Field>,
}

impl Decoder<f32> for FieldDecoder {
	fn decode(&mut self, bytes: &[u8], out: &mut Vec<f32>) {
		for field in &self.fields {
			out.push(field.decode(bytes).into());
		}
	}
}
impl FieldDecoder {
	pub fn from_fields_and_id(fields: &[Meta], ids: &[FieldId]) -> Self {
		let fields = fields
			.iter()
			.enumerate()
			.filter(|(i, _)| ids.contains(&(*i as u8)))
			.map(|(_, f)| f);
		FieldDecoder {
			fields: fields.map(|f| f.clone().into()).collect(),
		}
	}
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
	#[error("error accessing byteseries")]
	ByteSeries(#[from] byteseries::Error),
	#[error("io error")]
	Io(#[from] io::Error),
	#[error("syntax error in specification")]
	MalformedSpec,
}

impl DataSet {
	pub fn get_decode_info(&self, allowed_fields: &[FieldId]) -> SetSliceDecodeInfo {
		let mut offset_in_dataset = SmallVec::<[u8; 8]>::new();
		let mut lengths = SmallVec::<[u8; 8]>::new();
		let mut offset_in_recoded = SmallVec::<[u8; 8]>::new();

		let mut recoded_offset = 0;
		for id in allowed_fields {
			let field = &self.metadata.fields[*id as usize];
			offset_in_dataset.push(field.offset());
			lengths.push(field.length());
			offset_in_recoded.push(recoded_offset);
			recoded_offset += field.length();
		}

		SetSliceDecodeInfo {
			field_lenghts: lengths.into_vec(),
			field_offsets: offset_in_recoded.into_vec(),
			data_is_little_endian: cfg!(target_endian = "little"),
		}
	}

	pub fn get_update_uncompressed(
		&self,
		line: Vec<u8>,
		timestamp: i64,
		allowed_fields: &[FieldId],
		setid: DatasetId,
	) -> Vec<u8> {
		trace!("get_update_uncompressed");

		let mut recoded_line = SmallVec::<[u8; 64]>::new(); // initialize an empty vector

		//browsers tend to use little endian, thus present all data little endian
		debug!("setid: {}", setid);
		recoded_line.write_u16::<LittleEndian>(setid).unwrap();
		recoded_line
			.write_f64::<LittleEndian>(timestamp as f64)
			.unwrap();
		for field in allowed_fields
			.iter()
			.map(|id| &self.metadata.fields[*id as usize])
		{
			let decoded: f32 = field.decode(&line).into();
			recoded_line.write_f32::<LittleEndian>(decoded).unwrap();
		}
		recoded_line.to_vec()
	}
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq)]
pub enum Authorisation {
	Owner(FieldId),
	Reader(FieldId),
}

impl Ord for Authorisation {
	fn cmp(&self, other: &Self) -> Ordering {
		FieldId::from(self).cmp(&FieldId::from(other))
	}
}

impl PartialOrd for Authorisation {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		Some(self.cmp(other))
	}
}

impl PartialEq for Authorisation {
	fn eq(&self, other: &Self) -> bool {
		FieldId::from(self) == FieldId::from(other)
	}
}

impl std::hash::Hash for Authorisation {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        FieldId::from(self).hash(state);
    }
}

impl AsRef<FieldId> for Authorisation {
	fn as_ref(&self) -> &FieldId {
		match self {
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

pub struct Data {
	//TODO make multithreaded
	pub dir: PathBuf,
	free_dataset_id: u16,                  //replace with atomics
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
				if data_id + 1 > free_dataset_id {
					free_dataset_id = data_id + 1;
				}
				load_data(&mut sets, &path, data_id);
			}
		}
	}

	Ok(Data {
		dir,
		free_dataset_id,
		sets,
	})
}

pub fn load_data(data: &mut HashMap<DatasetId, DataSet>, datafile_path: &Path, data_id: DatasetId) {
	let mut info_path = datafile_path.to_owned();
	info_path.set_extension("yaml");
	if let Ok(metadata_file) = fs::OpenOptions::new()
		.read(true)
		.write(false)
		.create(false)
		.open(&info_path)
	{
		let metadata = serde_yaml::from_reader::<std::fs::File, FixedLine>(metadata_file)
            .expect(&format!("could not deserialise {:?}", info_path));
        let line_size = metadata.fieldsum();

        if let Ok(timeserie) = Series::open(datafile_path, line_size as usize) {
            info!("loaded dataset with id: {}", &data_id);
            data.insert(
                data_id,
                DataSet {
                    timeseries: timeserie,
                    metadata,
                },
            );
        }
	} else {
		warn!("could not open: {:?} for reading", info_path);
	}
}

impl Data {
	pub fn add_set<T: AsRef<Path>>(&mut self, spec_path: T) -> Result<DatasetId, Error> {
		let f = fs::OpenOptions::new()
			.read(true)
			.write(false)
			.create(false)
			.open(spec_path)?;
		let metadata =
			serde_yaml::from_reader::<File, MetaDataSpec>(f).map_err(|_| Error::MalformedSpec)?;
		let metadata: FixedLine = metadata.into();
		let line_size: u16 = metadata.fieldsum();
		let dataset_id = self.free_dataset_id;
		self.free_dataset_id += 1;
		let mut datafile_path = self.dir.clone();
		datafile_path.push(dataset_id.to_string());

		let set = DataSet {
			timeseries: Series::open(&datafile_path, line_size as usize)?,
			metadata,
		};
		datafile_path.set_extension("yaml");
		let f = fs::File::create(datafile_path).unwrap();
		serde_yaml::to_writer(f, &set.metadata).unwrap();

		self.sets.insert(dataset_id, set);
		info!("added timeseries under id: {}", dataset_id);
		Ok(dataset_id)
	}
}

impl Data {
	pub fn authenticate_error_packet(&mut self, data_string: &Bytes) -> Result<DatasetId, ()> {
		if data_string.len() < 12 {
			warn!(
				"error_string size (={}) to small for key, datasetid and any error (min 12 bytes)",
				data_string.len()
			);
			return Err(());
		}

		let dataset_id = LittleEndian::read_u16(&data_string[..2]);
		let key = LittleEndian::read_u64(&data_string[2..10]);

		if let Some(set) = self.sets.get_mut(&dataset_id) {
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

	pub fn store_new_data(
		&mut self,
		mut data_string: Bytes,
		time: DateTime<Utc>,
	) -> Result<(DatasetId, Vec<u8>), ()> {
		if data_string.len() < 11 {
			warn!(
				"data_string size (={}) to small for key, datasetid and any data (min 11 bytes)",
				data_string.len()
			);
			return Err(());
		}

		let dataset_id = LittleEndian::read_u16(&data_string[..2]);
		let key = LittleEndian::read_u64(&data_string[2..10]);

		if let Some(set) = self.sets.get_mut(&dataset_id) {
			if data_string.len() != set.metadata.fieldsum() as usize + 10 {
				warn!(
					"datastring has invalid length ({}) for node (id: {}), should have length: {}",
					data_string.len(),
					dataset_id,
					set.metadata.fieldsum() + 10
				);
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
					let decoded: f32 = field.decode(&data_string[10..]).into();
					list.push_str(&format!("{}: {}\n", field.name, decoded));
				}
				println!("{}", list);
			}

			if let Err(error) = set.timeseries.append(time, &data_string[10..]) {
				//if let Err(error) = set.timeseries.append_fast(time, &data_string[10..]){
				warn!("error on data append: {:?}", error);
				return Err(());
			}

			Ok((dataset_id, data_string.split_off(10).to_vec()))
		} else {
			warn!("could not find dataset with id: {}", dataset_id);
			Err(())
		}
	}
}
