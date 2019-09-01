
use chrono::offset::{TimeZone};
use chrono::{Date, Duration, Utc, DateTime};
use plotters::prelude::*;
use plotters::style::colors::{WHITE, BLACK, RED};
use plotters::coord::Shift;

use image::{png::PNGEncoder, RGB};
use log::{warn, error};

use crate::httpserver::{DataRouterState, InnerState, timeseries_interface};
use crate::databases::UserDbError;

use timeseries_interface::{FieldId, DatasetId, Data};
use timeseries_interface::read_to_array::{ReaderInfo, prepare_read_processing, read_into_arrays};

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Debug)]
pub enum Error{
    ArgumentError(std::num::ParseIntError),
    NoAccessToField(FieldId),
    NoAccessToDataSet(DatasetId),
    CouldNotSetupRead,
    NoDataWithinRange,
    PlotLibError,
    EncodingError(std::io::Error),
	BotDatabaseError(UserDbError),
}

impl From<std::num::ParseIntError> for Error {
	fn from(error: std::num::ParseIntError) -> Self {
		Error::ArgumentError(error)
	}
}

impl From<UserDbError> for Error {
	fn from(error: UserDbError) -> Self {
		Error::BotDatabaseError(error)
	}
}

pub fn plot(text: String, state: &DataRouterState, user_id: UserId)-> Result<Vec<u8>, Error>{

    let args = vec!(String::from("1"), 
        String::from("2"), 
        String::from("3"), 
        String::from("3"), 
        String::from("3")); //TODO actual args here

    let (timerange, set_id, field_ids) = parse_plot_arguments(args)?;
    let selected_datasets = select_data(state, set_id, field_ids, user_id)?;

    let dimensions = (900, 900);

    //Init plot
    let mut subpixelbuffer: Vec<u8> = Vec::new();
    let root = BitMapBackend::with_buffer(&mut subpixelbuffer, dimensions).into_drawing_area();
    //let root = BitMapBackend::new("sample2.png", (width, height)).into_drawing_area();
    root.fill(&WHITE).map_err(|_| Error::PlotLibError)?;

    let (to_date, from_date) = timerange;
    let (to_date, from_date) = (to_date.timestamp(), from_date.timestamp());

    let mut chart = ChartBuilder::on(&root)
        .x_label_area_size(40)
        .y_label_area_size(40)
        //.caption("MSFT Stock Price", ("Arial", 50.0).into_font())
        .build_ranged(from_date..to_date, 110f32..135f32)
        .map_err(|_| Error::PlotLibError)?;
    chart
        .configure_mesh()
        .line_style_2(&WHITE)
        .draw().map_err(|_| Error::PlotLibError)?;   

    //add lines
    for selected_data in selected_datasets {
        let (shared_x, mut y_datas, mut labels) = read_data(selected_data, &state.inner_state().data, timerange)?;
        for (mut y, label) in y_datas.drain(..).zip(labels.drain(..)) {
            chart.draw_series(LineSeries::new(
                shared_x.iter().map(|x| *x)
                .zip(y.drain(..)),
                &RED)
            ).map_err(|_| Error::PlotLibError)?
            .label(label.1);
        }
    }
    chart
        .configure_series_labels()
        .background_style(&WHITE.mix(0.8))
        .border_style(&BLACK)
        .draw().map_err(|_| Error::PlotLibError);

    drop(chart);
    drop(root);

    let mut image = Vec::new();
    PNGEncoder::new(&mut image)
        .encode(&subpixelbuffer, dimensions.0, dimensions.1, RGB(8))
        .map_err(|io_error| Error::EncodingError(io_error))?;

    return Ok(image);
}

fn parse_plot_arguments(args: Vec<String>)
     -> Result<((DateTime<Utc>, DateTime<Utc>), DatasetId, Vec<FieldId>), core::num::ParseIntError>{
    let timerange_start = Utc.timestamp(args[1].parse::<i64>()?/1000, (args[1].parse::<i64>()?%1000) as u32);
    let timerange_stop = Utc.timestamp(args[2].parse::<i64>()?/1000, (args[2].parse::<i64>()?%1000) as u32);
    let timerange = (timerange_start, timerange_stop);

    let set_id = args[3].parse::<timeseries_interface::DatasetId>()?;

    let field_ids = args[4..]
        .iter()
        .map(|arg| arg.parse::<timeseries_interface::FieldId>())
        .collect::<Result<Vec<timeseries_interface::FieldId>,std::num::ParseIntError>>()?;
    
    Ok((timerange, set_id, field_ids))
}

use super::UserId;
fn select_data(data: &DataRouterState, set_id: timeseries_interface::DatasetId, field_ids: Vec<FieldId>, user_id: UserId)
     -> Result<HashMap<DatasetId, Vec<FieldId>>,Error>{

    //get timeseries_with_access for this user
    let timeseries_with_access = data.bot_user_db.get_userdata(user_id)?.timeseries_with_access;

    let mut selected_data = HashMap::new();
    //check if user has access to the requested dataset
    if let Some(fields_with_access) = timeseries_with_access.get(&set_id){
        let mut subbed_fields = Vec::with_capacity(field_ids.len());
        for field_id in field_ids { 
            //prevent users requesting a field twice (this leads to an overflow later)
            if subbed_fields.contains(&field_id) {
                warn!("field was requested twice, ignoring duplicate");
            } else if fields_with_access.binary_search_by(|auth| auth.as_ref().cmp(&field_id)).is_ok() {
                subbed_fields.push(field_id);
            } else { 
                warn!("unautorised field requested");
                return Err(Error::NoAccessToField(field_id));
            }
        }
        selected_data.insert(set_id, subbed_fields);
    } else { 
        warn!("no access to dataset");
        return Err(Error::NoAccessToDataSet(set_id))
    }
    Ok(selected_data)
}

fn read_data(selected_data: (DatasetId, Vec<FieldId>), 
    data: &Arc<RwLock<Data>>, timerange: (DateTime<Utc>, DateTime<Utc>))
     -> Result<(Vec<i64>, Vec<Vec<f32>>,Vec<(FieldId, String)>),Error>{
    
    let max_plot_points = 1000;
    let (dataset_id, field_ids) = selected_data;

    let data_handle = data.clone();
    let mut data = data_handle.write().unwrap();

    let mut metadata = Vec::new();
    let dataset = data.sets.get_mut(&dataset_id).unwrap();
    if let Some(read_state) = dataset.prepare_read(timerange.0, timerange.1, &field_ids) {
        //prepare for reading and calc number of bytes we will be sending
        let n_lines = std::cmp::min(read_state.numb_lines, max_plot_points);
        if let Some(reader_info) = prepare_read_processing(
            read_state, &dataset.timeseries, max_plot_points, dataset_id) {

            //prepare and send metadata
            for field_id in field_ids.iter().map(|id| *id) {
                let field = &dataset.metadata.fields[field_id as usize];
                metadata.push( (field_id, field.name.to_owned()) );
            }

            std::mem::drop(data);
            let (x_shared, y_datas) = read_into_arrays(data_handle, reader_info);
            return Ok((x_shared, y_datas, metadata));
        
        
        } else { 
            error!("could not setup read");
            return Err(Error::CouldNotSetupRead);
        }
    } else { 
        warn!("no data within given window");
        return Err(Error::NoDataWithinRange);
    }
}