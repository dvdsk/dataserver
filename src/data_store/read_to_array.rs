use super::*;
use log::{warn};
use std::sync::{Arc,RwLock};
use minimal_timeseries::Selector;

//prepare read is taken from read.rs

///figures out what combination of: ignoring datapoints, averaging, read buffering and
///loosse packages should be used to fullfill a data request for a dataset
#[allow(dead_code)]
pub fn prepare_read_processing(read_state: ReadState, 
	timeseries: &minimal_timeseries::Timeseries, 
	max_points: u64, 
	dataset_id: DatasetId)
	 -> Option<ReaderInfo> {
	
	if max_points == 0 { warn!("cant create a plot with zero points"); return None; }

    //ideal read + ideal package size + ram per sample may not exceed free ram
    const IDEAL_READ_SIZE: usize = 1_000; //in bytes
    //const IDEAL_PACKAGE_SIZE: usize = 1_000;
    const HEADER_SIZE: usize = 4;
    let free_ram = 1_000_000; //1 mb

    let decoded_line_size = read_state.decoded_line_size;
    let selector = minimal_timeseries::Selector::new(
		max_points as usize, 
		read_state.numb_lines as u64, 
		&timeseries);

	//let lines_per_read = 

    Some(ReaderInfo{
        dataset_id,
        read_state,
        selector,
        lines_per_read: free_ram/(decoded_line_size),
        line_size: timeseries.line_size,

        timestamps: Vec::new(),
        line_data: Vec::new(),
    })
}

#[derive(Debug)]
pub struct ReaderInfo {
	dataset_id: DatasetId,
	read_state: ReadState,
	selector: Option<Selector>,
	lines_per_read: usize,
	line_size: usize, //size of line without timestamp

	timestamps: Vec<u64>,
	line_data: Vec<u8>,
}


fn create_vector_of_vectors(numb_vectors: usize) -> Vec<Vec<f32>>{
	let mut vec = Vec::new();
	for _ in 0..numb_vectors {
		vec.push(Vec::new());
	}
	return vec;
}

#[allow(dead_code)]
pub fn read_into_arrays(data_handle: Arc<RwLock<Data>>, mut reader: ReaderInfo)
	-> (Vec<i64>, Vec<Vec<f32>>) {
	
	let mut shared_x = Vec::new();
	let mut y_datas = create_vector_of_vectors(reader.read_state.fields.len());
	
	if reader.selector.is_none() { //numb_lines <= max_plot_points
		info!("reading complete dataset");
		let mut data = data_handle.write().unwrap(); //TODO per dataset then try_lock
		let dataset = data.sets.get_mut(&reader.dataset_id).unwrap();

		//read from file into a buffer
		dataset.timeseries.decode_time_into_given(
			&mut reader.timestamps,
			&mut reader.line_data,

			reader.lines_per_read, //max number of lines to read, is a multiple of lines_per_sample
			&mut reader.read_state.start_byte,
			reader.read_state.stop_byte,
			&mut reader.read_state.decode_params).expect("timeseries read returned no data");

		//drop and thus unlock the dataset
		std::mem::drop(dataset);
		std::mem::drop(data);

		decode_into_array(&mut reader, &mut shared_x, &mut y_datas);
		return (shared_x, y_datas);
	}

	while reader.read_state.start_byte < reader.read_state.stop_byte {
		let mut data = data_handle.write().unwrap(); //TODO per dataset then try_lock
		let dataset = data.sets.get_mut(&reader.dataset_id).unwrap();

		//read from file into a buffer
		dataset.timeseries.decode_time_into_given_skipping(
			&mut reader.timestamps,
			&mut reader.line_data,

			reader.lines_per_read, //max number of lines to read, is a multiple of lines_per_sample
			&mut reader.read_state.start_byte,
			reader.read_state.stop_byte,
			&mut reader.read_state.decode_params,
			reader.selector.as_mut().unwrap()).expect("timeseries read returned no data");

		//dbg!(&reader);

		//drop and thus unlock the dataset
		std::mem::drop(dataset);
		std::mem::drop(data);

		decode_into_array(&mut reader, &mut shared_x, &mut y_datas);
	}
	return (shared_x, y_datas);	
}

fn decode_into_array(reader: &mut ReaderInfo, shared_x: &mut Vec<i64>, y_datas: &mut Vec<Vec<f32>>) {

	let ReaderInfo {dataset_id, read_state, selector,
		lines_per_read, line_size, timestamps, line_data } = reader;
	
	let line_size = *line_size;
	let lines_per_sample = if let Some(sel) = selector {
		sel.lines_per_sample.get()
	} else {
		1
	};

	//add some lines
	for ts_sum in reader.timestamps
		.chunks_exact(lines_per_sample).map(|chunk| chunk.iter().sum::<u64>()) {
		let ts_avg =ts_sum/(lines_per_sample as u64);
		shared_x.push(ts_avg as i64);
	}//for every sample

	for sample in reader.line_data.chunks_exact(lines_per_sample* line_size) {
		let mut decoded_field_sums = vec!(0f32; read_state.fields.len()); //to store averages as they grow
		for line in sample.chunks_exact(line_size) {
			for (field, decoded_field) in read_state.fields.iter().zip(&mut decoded_field_sums) {
				let decoded: f32 = field.decode::<f32>(&line);
				*decoded_field += decoded;
				dbg!(decoded);
			}
		}
		for (decoded_sum,y) in decoded_field_sums.drain(..).zip(y_datas.iter_mut()){
			dbg!(decoded_sum);
			y.push(decoded_sum/lines_per_sample as f32);
		}
	}//for every sample
}