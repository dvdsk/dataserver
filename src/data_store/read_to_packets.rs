use super::*;
use log::{warn};
use std::sync::{Arc,RwLock, mpsc};
use std::mem;
use minimal_timeseries::Selector;

impl DataSet {
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
				if stop_byte-start_byte == 0 {return None; }

				let fields: Vec<Field<f32>> = requested_fields.into_iter()
				.map(|id| self.metadata.fields[*id as usize].clone() ).collect();

				let numb_lines = (stop_byte-start_byte)/(self.timeseries.full_line_size as u64) - 1;
				let decoded_line_size = fields.len()*std::mem::size_of::<f32>()+std::mem::size_of::<f64>();

				Some( ReadState {
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
}

	///figures out what combination of: ignoring datapoints, averaging, read buffering and
	///loosse packages should be used to fullfill a data request for a dataset
	pub fn prepare_read_processing(read_state: ReadState, timeseries: &minimal_timeseries::Timeseries, max_points: u64, dataset_id: DatasetId) -> Option<ReaderInfo> {
		//ideal read + ideal package size + ram per sample may not exceed free ram
		const IDEAL_READ_SIZE: usize = 1_000; //in bytes
		//const IDEAL_PACKAGE_SIZE: usize = 1_000;
		const HEADER_SIZE: usize = 4;
		let free_ram = 1_000_000; //1 mb
		let lines_in_range = read_state.numb_lines; //as u64

		if max_points == 0 { warn!("cant create a plot with zero points"); return None; }

		let line_size = timeseries.line_size;
		let full_line_size = timeseries.full_line_size;
		let decoded_line_size = read_state.decoded_line_size;

		let selector = minimal_timeseries::Selector::new(max_points as usize, lines_in_range as u64, &timeseries);
		let lines_per_sample = selector.as_ref().map_or(1, |x| x.lines_per_sample.get());

		//user always wants the max specified lines
		let requested_lines = u64::min(max_points, lines_in_range as u64);

		/*
		we decide the lines_per_package and lines_per_read based on: (decreasing importance)
			-both have to fit into ram available for decoding
			-send a minimum of 2 packages
			-reads and network packages should be bufferd
				-always use ideal read size for reading (should never give ram problems).
				-unless minimal numb of packages, size may not exceed ideal package size.
		*/


		//lines_per_package should be optimal for reading from disk
		let bytes_to_read_per_packet_line = lines_per_sample*(full_line_size as usize);
		let lines_per_package = usize::max(lines_per_sample, IDEAL_READ_SIZE/bytes_to_read_per_packet_line);

		//but more importantly we should send more then one package to allow work to be done parallel
		let lines_per_package = usize::min(lines_per_package, (requested_lines/3) as usize);

		//and most importantly do not use more ram then availible, we need at least the ram to hold
		//the package + space to decode one sample.
		let ram_per_package_line = lines_per_sample*full_line_size + decoded_line_size;
		let lines_per_package =	usize::min(lines_per_package, free_ram-ram_per_package_line);

		//theoretical additional contstraint, maximum u16::max packages (should never be reached)

		let lines_per_read = lines_per_package * lines_per_sample;
		let n_packages: u16 = (requested_lines/(lines_per_package as u64)) as u16;

		Some(ReaderInfo::new(
			dataset_id,
			n_packages,
			read_state,
			selector,
			lines_per_package*decoded_line_size+HEADER_SIZE, //bytes_per_package
			lines_per_read as usize,
			line_size,
		))
	}

#[derive(Debug)]
pub struct ReaderInfo {
	dataset_id: DatasetId,
	n_packages: u16,
	read_state: ReadState,
	selector: Option<Selector>,
	bytes_per_package: usize,
	lines_per_read: usize,
	package_numb: u16, //lowerd from n_packages to 1
	line_size: usize, //size of line without timestamp

	timestamps_buffer: Vec<u64>,
	line_data_buffer: Vec<u8>,

	package: Vec<u8>,
	done: bool,
}

impl ReaderInfo {
	fn new(dataset_id: DatasetId,
		n_packages: u16,
		read_state: ReadState,
		selector: Option<Selector>,
		bytes_per_package: usize,
		lines_per_read: usize,
		line_size: usize) -> Self {

		let first_package_numb = n_packages+1;
		ReaderInfo {
			dataset_id,
			n_packages,
			package_numb: first_package_numb-1,
			read_state,
			selector,
			bytes_per_package, // < MAX_PACKAGE_SIZE/decoded_line_size
			lines_per_read, //must be multiple of sample_size
			line_size,

			timestamps_buffer: Vec::with_capacity(lines_per_read),
			line_data_buffer: Vec::with_capacity(lines_per_read*line_size),
			package: Self::new_package(first_package_numb, dataset_id, bytes_per_package),
			done: false,
		}
	}
}

pub fn read_into_packages(data_handle: Arc<RwLock<Data>>, mut tx: mpsc::SyncSender<Vec<u8>>, mut reader_infos: Vec<ReaderInfo>){
	let mut sets_to_process =	reader_infos.len();

	//for small reader
	for mut reader in reader_infos.iter_mut().filter(|x| x.selector.is_none() ) {
		dbg!("HANDELING NON SELECTOR READ");
		let mut data = data_handle.write().unwrap(); //TODO per dataset then try_lock
		let dataset = data.sets.get_mut(&reader.dataset_id).unwrap();

		//read from file into a buffer
		dataset.timeseries.decode_time_into_given(
			&mut reader.timestamps_buffer,
			&mut reader.line_data_buffer,

			reader.lines_per_read, //max number of lines to read, is a multiple of lines_per_sample
			&mut reader.read_state.start_byte,
			reader.read_state.stop_byte,
			&mut reader.read_state.decode_params).expect("timeseries read returned no data");

		//drop and thus unlock the dataset
		std::mem::drop(dataset);
		std::mem::drop(data);
		//dbg!(&reader);
		let lines_per_sample = 1;
		match reader.empty_into_packages(&mut tx, lines_per_sample){
			PackageResult::BufferEmptied => continue,
			PackageResult::ConnectionDropped => return,
			PackageResult::LastQueued => {
				reader.done =	true; //mark reader as done
				sets_to_process -= 1;
			},
		}
	}

	while sets_to_process > 0 {
		for mut reader in reader_infos.iter_mut().filter(|x| !x.done) {
			let mut data = data_handle.write().unwrap(); //TODO per dataset then try_lock
			let dataset = data.sets.get_mut(&reader.dataset_id).unwrap();

			//read from file into a buffer
			dataset.timeseries.decode_time_into_given_skipping(
				&mut reader.timestamps_buffer,
				&mut reader.line_data_buffer,

				reader.lines_per_read, //max number of lines to read, is a multiple of lines_per_sample
				&mut reader.read_state.start_byte,
				reader.read_state.stop_byte,
				&mut reader.read_state.decode_params,
				reader.selector.as_mut().unwrap()).expect("timeseries read returned no data");


			//dbg!(&reader);

			//drop and thus unlock the dataset
			std::mem::drop(dataset);
			std::mem::drop(data);

			//dbg!(&reader.timestamps_buffer.len());
			let lines_per_sample = reader.selector.as_mut().unwrap().lines_per_sample.get();
			match reader.empty_into_packages(&mut tx, lines_per_sample){
				PackageResult::BufferEmptied => continue,
				PackageResult::ConnectionDropped => return,
				PackageResult::LastQueued => {
					reader.done =	true; //mark reader as done
					sets_to_process -= 1;
				},
			}

		}//for reader in readers
	}//while reader_infos not empty
}

enum PackageResult {
	BufferEmptied, //new package number contained
	LastQueued, //all data has been send
	ConnectionDropped,
}

impl ReaderInfo {
	///writes buffers into packages which are send queued for sending until buffer is empty
	/// //TODO remove lines_per_sample should get it from "self"
	fn empty_into_packages(&mut self, tx: &mut mpsc::SyncSender<Vec<u8>>, lines_per_sample: usize) -> PackageResult {
		let decoded_line_size = self.read_state.decoded_line_size;
		let mut tstamp_remainder = self.timestamps_buffer.as_slice();
		let mut ldate_remainder = self.line_data_buffer.as_slice();

		//FIXME TODO switch from redefines to incrementive

		//figure out how much extra data can fit into the current package
		//dbg!(self.bytes_per_package);
		//dbg!(self.package.len());
		//dbg!(lines_per_sample);
		
		let mut bytes_to_fill_package = (self.bytes_per_package - self.package.len())*lines_per_sample;
		let mut lines_to_fill_package = bytes_to_fill_package /decoded_line_size; //TODO move into struct?

		//dbg!(ldate_remainder.len());
		//dbg!(bytes_to_fill_package);

		//FIXME when does this exit? how can that exit fail?
		//while there is more data available then fits into the current package,
		//put all the data into a package send it off then create a fresh package.
		while ldate_remainder.len() >= bytes_to_fill_package {
			//dbg!(lines_per_sample);
			//dbg!(tstamp_remainder.len());
			//dbg!(leftover_package_space*lines_per_sample);
			let (tstamp_left, new_tstamp_remainder) = tstamp_remainder
				.split_at(lines_to_fill_package);
			let (ldata_left, new_ldate_remainder) = ldate_remainder
				.split_at(bytes_to_fill_package);
			tstamp_remainder = new_tstamp_remainder;
			ldate_remainder = new_ldate_remainder;

			bytes_to_fill_package = self.bytes_per_package*lines_per_sample; //FIXME recalc not needed after first loop
			lines_to_fill_package = bytes_to_fill_package /decoded_line_size;

			//self.decode_into_package(tstamp_left, ldata_left);
			Self::decode_into_package(tstamp_left, ldata_left, &mut self.package, self.line_size,
			                          &self.read_state, lines_per_sample);

			let next_package = Self::new_package(self.package_numb, self.dataset_id, self.bytes_per_package);//prepare next package
			//replace self.package with next package and send the old self.package
			if tx.send(mem::replace(&mut self.package, next_package)).is_err() {
				return PackageResult::ConnectionDropped
			}
			if self.package_numb == 0 {
				return PackageResult::LastQueued
			}
			self.package_numb -= 1; //set package number for next package
		}
		//self.decode_into_package(tstamp_remainder, ldate_remainder);

		Self::decode_into_package(tstamp_remainder, ldate_remainder, &mut self.package, self.line_size,
			                        &self.read_state, lines_per_sample);
		//still need a new package for the next read or the next dataset
		let next_package = Self::new_package(self.package_numb, self.dataset_id, self.bytes_per_package);//prepare next package
		//replace self.package with next package and send the old self.package
		if tx.send(mem::replace(&mut self.package, next_package)).is_err() {
			return PackageResult::ConnectionDropped
		}
		if self.package_numb == 0 {
			return PackageResult::LastQueued
		}
		self.package_numb -= 1; //set package number for next package
		//TODO what if we hit last package here?

		PackageResult::BufferEmptied
	}

	fn new_package(package_numb: u16, dataset_id: DatasetId, bytes_per_package: usize) -> Vec<u8>{
		let mut package = Vec::with_capacity(bytes_per_package);
		package.write_u16::<LittleEndian>(package_numb).unwrap();
		package.write_u16::<LittleEndian>(dataset_id).unwrap();
		package.write_u32::<LittleEndian>(0).unwrap(); //add needed padding
		package
	}

	// fn decode_into_package(&mut self, tstamps: &[u64], ldata: &[u8]) {
		//add some lines
	// 	let sample_size = self.selector.lines_per_sample.get();
	// 	for ts_sum in tstamps
	// 		.chunks_exact(sample_size).map(|chunk| chunk.iter().sum::<u64>()) {

	// 		let ts_avg =ts_sum/(sample_size as u64);
	// 		self.package.write_f64::<LittleEndian>(ts_avg as f64).unwrap();
	// 	}//for every sample

	// 	for sample in ldata.chunks_exact(sample_size*self.line_size) {
	// 		let mut decoded_fields = vec!(0f32; self.read_state.fields.len()); //to store averages as they grow
	// 		for line in sample.chunks_exact(self.line_size) {
	// 			for (field, decoded_field) in self.read_state.fields.iter().zip(&mut decoded_fields) {
	// 				let decoded: f32 = field.decode::<f32>(&line);
	// 				*decoded_field += decoded;
	// 			}
	// 		}
	// 		for decoded in decoded_fields.drain(..){
	// 			self.package.write_f32::<LittleEndian>(decoded).unwrap();
	// 		}
	// 	}//for every sample
	// }

	fn decode_into_package(tstamps: &[u64], ldata: &[u8], package: &mut Vec<u8>, line_size: usize, read_state: &ReadState, lines_per_sample: usize) {

		//add some lines
		//dbg!(lines_per_sample);
		for ts_sum in tstamps
			.chunks_exact(lines_per_sample).map(|chunk| chunk.iter().sum::<u64>()) {
			let ts_avg =ts_sum/(lines_per_sample as u64);
			package.write_f64::<LittleEndian>(ts_avg as f64).unwrap();
		}//for every sample

		for sample in ldata.chunks_exact(lines_per_sample*line_size) {
			let mut decoded_field_sums = vec!(0f32; read_state.fields.len()); //to store averages as they grow
			for line in sample.chunks_exact(line_size) {
				for (field, decoded_field) in read_state.fields.iter().zip(&mut decoded_field_sums) {
					let decoded: f32 = field.decode::<f32>(&line);
					*decoded_field += decoded;
				}
			}
			for decoded_sum in decoded_field_sums.drain(..){
				package.write_f32::<LittleEndian>(decoded_sum/lines_per_sample as f32).unwrap();//FIXME actually do an average here
			}
		}//for every sample
	}
}

//TODO fix timestamps (they seem incorrect for every new package??? (inspect manually on js side)
//package size not so good (misses one timestamp and one line) 12 bytes normally per package
