
use chrono::offset::{TimeZone};
use chrono::{Date, Duration, Utc, DateTime, NaiveDateTime};
use plotters::prelude::*;
use plotters::style::colors::{WHITE, BLACK, RED};
use plotters::coord::Shift;

use image::{png::PNGEncoder, RGB};
use log::{warn, error};

use crate::data_store::Data;
use crate::data_store::{DatasetId, FieldId};
use crate::databases::{BotUserInfo, UserDbError};
use crate::data_store::data_router::DataRouterState;
use crate::data_store::read_to_array::{ReaderInfo, prepare_read_processing, read_into_arrays};

use crate::bot::Error as botError;
use telegram_bot::types::refs::{ChatId, UserId};

use std::collections::HashMap;
use std::sync::{Arc, RwLock};


#[derive(Debug)]
pub enum Error{
    ArgumentParseErrorF(std::num::ParseFloatError),
    ArgumentParseErrorI(std::num::ParseIntError),
    IncorrectArgument(String),
    NoAccessToField(FieldId),
    NoAccessToDataSet(DatasetId),
    CouldNotSetupRead(DatasetId, Vec<FieldId>),
    NoDataWithinRange,
    PlotLibError,
    EncodingError(std::io::Error),
	BotDatabaseError(UserDbError),
    NotEnoughArguments,
}

pub const USAGE: &str = "/plot <plotable_id> <number><s|m|h|d|w|monthes|years>";
pub const DESCRIPTION: &str = "send a line graph of a sensor value aka plotable, \
 from a given time ago till now. Optionally adding \"-s\" <start:stop> allows to \
 specify the start and stop value for the y-axis";
impl Error {
    pub fn to_text(self, user_id: UserId) -> String {
        match self {
            Error::ArgumentParseErrorF(_) => format!("One of the arguments could not be converted to a number\nuse: {}", USAGE),
            Error::ArgumentParseErrorI(_) => format!("One of the arguments could not be converted to a number\nuse: {}", USAGE),
            Error::IncorrectArgument(arg) => format!("Incorrectly formatted argument: \"{}\"\nuse: {}", arg, USAGE),
            Error::NoAccessToField(field_id) => format!("You do not have access to field: {}", field_id),
            Error::NoAccessToDataSet(dataset_id) => format!("You do not have access to dataset: {}", dataset_id),
            Error::CouldNotSetupRead(dataset_id, fields) => {
                error!("could not setup read for dataset {} and fields {:?}", dataset_id, fields);
                String::from("Apologies an internal error occured, it has been reported")
            },
            Error::NoDataWithinRange => String::from("I have no data between the times you requested"),
            Error::PlotLibError => {
                error!("internal error in plotting lib");
                String::from("Apologies an internal error occured, I have reported it")
            },
            Error::EncodingError(error) => {
                error!("could not encode png: {}", error);
                String::from("Apologies an internal error occured, I have reported it")
            },
            Error::BotDatabaseError(db_error) => db_error.to_text(user_id),
            Error::NotEnoughArguments => format!("Not enough arguments\nuse: {}", USAGE),            
        }
    }
}

impl From<std::num::ParseIntError> for Error {
	fn from(error: std::num::ParseIntError) -> Self {
		Error::ArgumentParseErrorI(error)
	}
}

impl From<std::num::ParseFloatError> for Error {
	fn from(error: std::num::ParseFloatError) -> Self {
		Error::ArgumentParseErrorF(error)
	}
}

impl From<UserDbError> for Error {
	fn from(error: UserDbError) -> Self {
		Error::BotDatabaseError(error)
	}
}

pub fn send(chat_id: ChatId, user_id: UserId, state: &DataRouterState, token: &str, 
    args: std::str::SplitWhitespace<'_>, userinfo: &BotUserInfo) -> Result<(), botError>{

	let args: Vec<String> =	args.map(|s| s.to_owned() ).collect();
	let plot = plot(args, state, user_id, userinfo)?;

	let photo_part = reqwest::multipart::Part::bytes(plot)
		.mime_str("image/png").unwrap()
		.file_name("testplot.png");

	let url = format!("https://api.telegram.org/bot{}/sendPhoto", token);

	let form = reqwest::multipart::Form::new()
		.text("chat_id", chat_id.to_string())
		.part("photo", photo_part);

	let client = reqwest::Client::new();
	let resp = client.post(&url)
		.multipart(form).send()?;
	
	if resp.status() != reqwest::StatusCode::OK {
		Err(botError::InvalidServerResponse(resp))
	} else {
		Ok(())
	}
}

fn plot(args: Vec<String>, state: &DataRouterState, user_id: UserId, userinfo: &BotUserInfo)
    -> Result<Vec<u8>, Error>{

    let (timerange, set_id, field_id, scaling) = parse_plot_arguments(args)?;
    let selected_datasets = select_data(state, set_id, vec!(field_id), userinfo)?;
    let dimensions = (900u32, 900u32);

    //Init plot
    let mut subpixelbuffer: Vec<u8> = vec!(0u8;(dimensions.0*dimensions.1*3) as usize);
    let root = BitMapBackend::with_buffer(&mut subpixelbuffer, dimensions)
        .into_drawing_area();
    root.fill(&WHITE).map_err(|_| Error::PlotLibError)?;

    let (from_date, to_date) = timerange;
    //let (to_date, from_date) = (to_date.timestamp(), from_date.timestamp()); 
    let (y_min, y_max) = if let Some(manual) = scaling {
        manual
    } else {
        (0f32,40f32) //TODO replace with auto
    };

    dbg!(to_date);
    dbg!(from_date);
    dbg!(from_date..to_date);
    let mut chart = ChartBuilder::on(&root)
        .x_label_area_size(40)
        .y_label_area_size(40)
        .build_ranged(from_date..to_date, y_min..y_max)
        .map_err(|_| Error::PlotLibError)?;
    chart //Causes crash (div zero), or takes forever, need to solve 
        .configure_mesh()
        .line_style_2(&WHITE)
        .draw().map_err(|_| Error::PlotLibError)?;

    //add lines
    for selected in selected_datasets {
        let (shared_x, mut y_datas, mut labels) = read_data(selected, &state.data, timerange)?;
        for (mut y, label) in y_datas.drain(..).zip(labels.drain(..)) {
            chart.draw_series(LineSeries::new(
                shared_x.iter()
                    //.map(|x|*x)
                    .map(|x| DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(*x, 0), Utc))
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
        .draw().map_err(|_| Error::PlotLibError)?;

    drop(chart);
    drop(root);

    let mut image = Vec::new();
    PNGEncoder::new(&mut image)
        .encode(&subpixelbuffer, dimensions.0, dimensions.1, RGB(8))
        .map_err(|io_error| Error::EncodingError(io_error))?;

    return Ok(image);
}

fn parse_plot_arguments(args: Vec<String>)
     -> Result<((DateTime<Utc>, DateTime<Utc>), DatasetId, FieldId, Option<(f32,f32)>), Error>{
    
    if args.len() < 2 {return Err(Error::NotEnoughArguments);}

    let mut plotable = args[0].split(":");
    let set_id = plotable.nth(0)
        .ok_or(Error::IncorrectArgument(args[0].clone()))?
        .parse::<DatasetId>()?;
    
    let field_id = plotable.next()      
        .ok_or(Error::IncorrectArgument(args[0].clone()))?
        .parse::<FieldId>()?;


    let end = args[1].find(char::is_alphabetic)
        .ok_or(Error::IncorrectArgument(args[1].clone()))?;
    
    let numb =  args[1][..end].parse::<i64>()?;
    let unit = &args[1][end..];
    let duration = match unit {
        "s" => Duration::seconds(numb),
        "m" => Duration::minutes(numb),
        "h" => Duration::hours(numb),
        "d" => Duration::days(numb),
        "w" => Duration::weeks(numb),
        "monthes" => Duration::weeks(4*numb),
        "years" => Duration::days(365*numb),
        _ => return Err(Error::IncorrectArgument(args[1].clone())),
    };
    let timerange = (Utc::now() - duration, Utc::now());

    //optional argument
    let mut scaling = if args.len() > 2 {
        let mut params = args[2].split(":");
        let y_min = params.nth(0)
            .ok_or(Error::IncorrectArgument(args[2].clone()))?
            .parse::<f32>()?;
        let y_max = params.next()
            .ok_or(Error::IncorrectArgument(args[2].clone()))?
            .parse::<f32>()?;
        Some((y_min, y_max))
    } else {
        None
    };
    dbg!();
    Ok((timerange, set_id, field_id, scaling))
}

pub fn select_data(data: &DataRouterState, set_id: DatasetId, field_ids: Vec<FieldId>, userinfo: &BotUserInfo)
     -> Result<HashMap<DatasetId, Vec<FieldId>>,Error>{

    //get timeseries_with_access for this user
    let timeseries_with_access = &userinfo.timeseries_with_access;

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

    let data_handle = data;
    let mut data = data_handle.write().unwrap();

    let mut metadata = Vec::new();
    let dataset = data.sets.get_mut(&dataset_id).unwrap();
    if let Some(read_state) = dataset.prepare_read(timerange.0, timerange.1, &field_ids) {
        //prepare for reading and calc number of bytes we will be sending
        let n_lines = std::cmp::min(read_state.numb_lines, max_plot_points);
        if let Some(reader_info) = prepare_read_processing(
            read_state, &dataset.timeseries, max_plot_points, dataset_id) {

            //prepare metadata
            for field_id in field_ids.iter().map(|id| *id) {
                let field = &dataset.metadata.fields[field_id as usize];
                metadata.push( (field_id, field.name.to_owned()) );
            }
            std::mem::drop(data);

            let (x_shared, y_datas) = read_into_arrays(data_handle.clone(), reader_info);
            return Ok((x_shared, y_datas, metadata));
        
        
        } else { 
            error!("could not setup read");
            return Err(Error::CouldNotSetupRead(dataset_id, field_ids));
        }
    } else { 
        warn!("no data within given window");
        return Err(Error::NoDataWithinRange);
    }
}