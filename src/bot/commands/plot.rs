use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use plotters::prelude::*;
use plotters::style::colors::{BLACK, RED, WHITE};

use image::{png::PngEncoder, ColorType};
use log::{error, warn};

use crate::data_store::data_router::DataRouterState;
use crate::data_store::{Data, DatasetId, FieldDecoder};
use crate::databases::{User, UserDbError};
use bitspec::FieldId;

use crate::bot::Error as botError;
use telegram_bot::types::refs::ChatId;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub const USAGE: &str = "/plot <plotable_id> <number><s|m|h|d|w|monthes|years>";
pub const DESCRIPTION: &str = "send a line graph of a sensor value aka plotable, \
 from a given time ago till now. Optionally adding <start:stop> allows to \
 specify the start and stop value for the y-axis";

#[derive(thiserror::Error, Debug)]
pub enum Error {
	#[error(
		"One of the arguments could not be converted to a number\nuse: {}",
		USAGE
	)]
	ArgumentParseErrorF(#[from] std::num::ParseFloatError),
	#[error(
		"One of the arguments could not be converted to a number\nuse: {}",
		USAGE
	)]
	ArgumentParseErrorI(#[from] std::num::ParseIntError),
	#[error("Incorrectly formatted argument: \"{0}\"\nuse: {}", USAGE)]
	IncorrectArgument(String),
	#[error("Incorrectly formatted argument: \"{0}\"\nuse: {}", USAGE)]
	NoAccessToField(FieldId),
	#[error("You do not have access to field: {0}")]
	NoAccessToDataSet(DatasetId),
	#[error("internal error in plotting lib")]
	PlotLibError,
	#[error("could not encode png: {0}")]
	EncodingError(image::error::ImageError),
	#[error("internal db error")]
	BotDatabaseError(#[from] UserDbError),
	#[error("Not enough arguments \nuse: {}", USAGE)]
	NotEnoughArguments,
	#[error("Internal error regarding the limits of the data")] //TODO remove if no longer issue
	DataLimitsAlarm,
	#[error("Error getting data: {0}")]
	DatasetError(#[from] byteseries::Error),
}

pub async fn send(
	chat_id: ChatId,
	state: &DataRouterState,
	token: &str,
	args: String,
	user: &User,
) -> Result<(), botError> {
	let args: Vec<String> = args.split_whitespace().map(|s| s.to_owned()).collect();
	let plot = plot(args, state, user)?;

	let photo_part = reqwest::multipart::Part::bytes(plot)
		.mime_str("image/png")
		.unwrap()
		.file_name("testplot.png");

	let url = format!("https://api.telegram.org/bot{}/sendPhoto", token);

	let form = reqwest::multipart::Form::new()
		.text("chat_id", chat_id.to_string())
		.part("photo", photo_part);

	let client = reqwest::Client::new();
	let resp = client.post(&url).multipart(form).send().await?;

	if resp.status() != reqwest::StatusCode::OK {
		Err(botError::InvalidServerResponse(resp))
	} else {
		Ok(())
	}
}

type PlotData = (Vec<i64>, Vec<f32>, Vec<(FieldId, String)>);
fn xlimits_from_data(data: &Vec<PlotData>) -> Result<(DateTime<Utc>, DateTime<Utc>), Error> {
	let mut min_ts = data.iter().map(|d| *d.0.first().unwrap()).min().unwrap();
	let mut max_ts = data.iter().map(|d| *d.0.last().unwrap()).max().unwrap();

	if min_ts > max_ts {
		dbg!(min_ts); //min is correct max is wrong
		dbg!(max_ts);

		for times in data.iter().map(|d| &d.0) {
			dbg!(&times[times.len() - 5..]);
			dbg!(&times[..5]);
		}

		return Err(Error::DataLimitsAlarm);
	}

	if min_ts == max_ts {
		min_ts -= 1;
		max_ts += 1;
	}

	let min = DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(min_ts, 0), Utc);
	let max = DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(max_ts, 0), Utc);
	Ok((min, max))
}

fn ylimits_from_data(data: &Vec<PlotData>) -> (f32, f32) {
	let mut min = *data
		.iter()
		.map(|data_set| {
			data_set
				.1
				.iter()
				.min_by(|a, b| a.partial_cmp(b).expect("NAN in plot data"))
				.unwrap()
		})
		.min_by(|a, b| a.partial_cmp(b).expect("NAN in plot data"))
		.unwrap();
	let mut max = *data
		.iter()
		.map(|data_set| {
			data_set
				.1
				.iter()
				.max_by(|a, b| a.partial_cmp(b).expect("NAN in plot data"))
				.unwrap()
		})
		.max_by(|a, b| a.partial_cmp(b).expect("NAN in plot data"))
		.unwrap();

	if min == max {
		min -= 0.1;
		max += 0.1;
	} else {
		min -= 0.025 * (max - min);
		max += 0.025 * (max - min)
	}
	(min, max)
}

fn format_str_from_limits(from: &DateTime<Utc>, to: &DateTime<Utc>) -> &'static str {
	let duration = *to - *from;

	if duration < Duration::minutes(1) {
		"%Ss"
	} else if duration < Duration::minutes(15) {
		"%Mm:%Ss"
	} else if duration < Duration::hours(1) {
		"%Mm"
	} else if duration < Duration::days(1) {
		"%Hh:%Mm"
	} else if duration < Duration::weeks(2) {
		"%a %Hh"
	} else if duration < Duration::weeks(5) {
		"%d-%m"
	} else {
		"%v"
	}
}

fn plot(args: Vec<String>, state: &DataRouterState, user: &User) -> Result<Vec<u8>, Error> {
	const DIMENSIONS: (u32, u32) = (900u32, 900u32);
	let (timerange, set_id, field_id, scaling_args) = parse_plot_arguments(args)?;
	let selected_datasets = select_data(set_id, vec![field_id], user)?;

	//collect data for plotting
	let plot_data: Result<Vec<PlotData>, Error> = selected_datasets
		.into_iter() //.filter_map(Result::ok)
		.map(|sel| read_data(sel, &state.data, timerange))
		.collect();
	let plot_data = plot_data?;

	let (from_date, to_date) = xlimits_from_data(&plot_data)?;
	dbg!(&from_date, &to_date);
	let x_label_formatstr = format_str_from_limits(&from_date, &to_date);
	let (y_min, y_max) = if let Some(manual) = scaling_args {
		manual
	} else {
		ylimits_from_data(&plot_data)
	};

	//Init plot
	let mut subpixelbuffer: Vec<u8> = vec![0u8; (DIMENSIONS.0 * DIMENSIONS.1 * 3) as usize];
	let root = BitMapBackend::with_buffer(&mut subpixelbuffer, DIMENSIONS).into_drawing_area();
	root.fill(&WHITE).map_err(|_| Error::PlotLibError)?;

	dbg!(y_min, y_max);
	let mut chart = ChartBuilder::on(&root)
		.x_label_area_size(40)
		.y_label_area_size(40)
		.build_ranged(from_date..to_date, y_min..y_max)
		.map_err(|_| Error::PlotLibError)?;
	chart //Causes crash (div zero), or takes forever, need to solve
		.configure_mesh()
		.line_style_2(&WHITE)
		.x_label_formatter(&|v| v.format(x_label_formatstr).to_string())
		.draw()
		.map_err(|_| Error::PlotLibError)?;

	//add lines
	for (shared_x, ys, mut labels) in plot_data {
        let n_lines = labels.len();
		for (i, label) in labels.drain(..).enumerate() {
			chart
				.draw_series(LineSeries::new(
					shared_x.iter().map(|x| {
							DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(*x, 0), Utc)
						})
						.zip(ys.iter().skip(i).step_by(n_lines).copied()),
					&RED,
				))
				.map_err(|_| Error::PlotLibError)?
				.label(label.1);
		}
	}
	//finish plot
	chart
		.configure_series_labels()
		.background_style(&WHITE.mix(0.8))
		.border_style(&BLACK)
		.draw()
		.map_err(|_| Error::PlotLibError)?;

	drop(chart);
	drop(root);

	//plot to png image
	let mut image = Vec::new();
	PngEncoder::new(&mut image)
		.encode(&subpixelbuffer, DIMENSIONS.0, DIMENSIONS.1, ColorType::Rgb8)
		.map_err(|io_error| Error::EncodingError(io_error))?;

	return Ok(image);
}

fn parse_plot_arguments(
	args: Vec<String>,
) -> Result<
	(
		(DateTime<Utc>, DateTime<Utc>),
		DatasetId,
		FieldId,
		Option<(f32, f32)>,
	),
	Error,
> {
	if args.len() < 2 {
		return Err(Error::NotEnoughArguments);
	}

	let mut plotable = args[0].split("_");
	let set_id = plotable
		.nth(0)
		.ok_or(Error::IncorrectArgument(args[0].clone()))?
		.parse::<DatasetId>()?;

	let field_id = plotable
		.next()
		.ok_or(Error::IncorrectArgument(args[0].clone()))?
		.parse::<FieldId>()?;

	let end = args[1]
		.find(|c: char| c.is_alphabetic() || c == '.')
		.ok_or(Error::IncorrectArgument(args[1].clone()))?;

	let numb = args[1][..end].parse::<f32>()?;
	let unit = &args[1][end..];
	let duration = match unit {
		"s" => numb * (1) as f32,
		"m" => numb * (60) as f32,
		"h" => numb * (3600) as f32,
		"d" => numb * (24 * 3600) as f32,
		"w" => numb * (7 * 24 * 3600) as f32,
		"monthes" => numb * (4 * 7 * 24 * 3600) as f32,
		"years" => numb * (365 * 24 * 3600) as f32,
		_ => return Err(Error::IncorrectArgument(args[1].clone())),
	};
	let duration = Duration::seconds(duration as i64);
	let timerange = (Utc::now() - duration, Utc::now());

	//optional argument
	let scaling = if args.len() > 2 {
		let mut params = args[2].split(":");
		let y_min = params
			.nth(0)
			.ok_or(Error::IncorrectArgument(args[2].clone()))?
			.parse::<f32>()?;
		let y_max = params
			.next()
			.ok_or(Error::IncorrectArgument(args[2].clone()))?
			.parse::<f32>()?;
		if y_min == y_max {
			return Err(Error::IncorrectArgument(args[2].clone()));
		}

		Some((y_min, y_max))
	} else {
		None
	};
	dbg!();
	Ok((timerange, set_id, field_id, scaling))
}

pub fn select_data(
	set_id: DatasetId,
	field_ids: Vec<FieldId>,
	user: &User,
) -> Result<HashMap<DatasetId, Vec<FieldId>>, Error> {
	//get timeseries_with_access for this user
	let timeseries_with_access = &user.timeseries_with_access;

	let mut selected_data = HashMap::new();
	//check if user has access to the requested dataset
	if let Some(fields_with_access) = timeseries_with_access.get(&set_id) {
		let mut subbed_fields = Vec::with_capacity(field_ids.len());
		for field_id in field_ids {
			//prevent users requesting a field twice (this leads to an overflow later)
			if subbed_fields.contains(&field_id) {
				warn!("field was requested twice, ignoring duplicate");
			} else if fields_with_access
				.binary_search_by(|auth| auth.as_ref().cmp(&field_id))
				.is_ok()
			{
				subbed_fields.push(field_id);
			} else {
				warn!("unautorised field requested");
				return Err(Error::NoAccessToField(field_id));
			}
		}
		selected_data.insert(set_id, subbed_fields);
	} else {
		warn!("no access to dataset");
		return Err(Error::NoAccessToDataSet(set_id));
	}
	Ok(selected_data)
}

fn read_data(
	selected_data: (DatasetId, Vec<FieldId>),
        data: &Arc<RwLock<Data>>,
	timerange: (DateTime<Utc>, DateTime<Utc>),
) -> Result<PlotData, Error> {
	let max_plot_points = 1000;
	let (dataset_id, field_ids) = selected_data;

	let data_handle = data;
	let mut data = data_handle.write().unwrap();

	let dataset = data.sets.get_mut(&dataset_id).unwrap();

	let fields = &dataset.metadata.fields;
	let mut decoder = FieldDecoder::from_fields_and_id(fields, &field_ids);
	let mut sampler = byteseries::new_sampler(&dataset.timeseries, &mut decoder)
		.start(timerange.0)
		.stop(timerange.1)
		.points(max_plot_points)
		.build()?;

	sampler.sample_all()?; //TODO some sampling over a mean probably wise
	let (x_shared, ys) = sampler.into_data();

	//prepare metadata
	let mut metadata = Vec::new();
	for field_id in field_ids.iter().map(|id| *id) {
		let field = &dataset.metadata.fields[field_id as usize];
		metadata.push((field_id, field.name.to_owned()));
	}

	Ok((x_shared, ys, metadata))
}
