pub const USAGE: &str = "/show <plotable_id 1> ... <plotable_id n>";
pub const DESCRIPTION: &str = "sends the current value(s) of the requested plotable(s)";

use chrono::{DateTime, Utc};
use std::collections::HashMap;

use telegram_bot::types::refs::ChatId;

use crate::data_store::data_router::DataRouterState;
use crate::data_store::{DatasetId, FieldDecoder};
use crate::databases::{User, UserDbError};
use bitspec::FieldId;

use super::super::send_text_reply;
use super::super::Error as botError;

#[derive(thiserror::Error, Debug)]
pub enum Error {
	#[error("The argument {0} could not be parsed")]
	ArgumentParseError(String, std::num::ParseIntError),
	#[error("The argument {0} could not be interpreted, does it contain a \":\"?")]
	ArgumentSplitError(String),
	#[error("You do not have access to field: {0}")]
	NoAccessToField(FieldId),
	#[error("You do not have access to dataset: {0}")]
	NoAccessToDataSet(DatasetId),
	#[error("There is no data for dataset: {0}")]
	NoData(DatasetId),
	#[error("database error")]
	BotDatabaseError(#[from] UserDbError),
	#[error("Not enough arguments\nuse: {}", USAGE)]
	NotEnoughArguments,
	#[error("Error accessing dataset: {0}")]
	DataSetError(#[from] byteseries::Error),
}

fn parse_args(args: String, user: &User) -> Result<Vec<(DatasetId, Vec<FieldId>)>, Error> {
	//keep a list of fields for each dataset
	let mut dataset_fields: HashMap<DatasetId, Vec<FieldId>> = HashMap::new();

	for arg in args.split_whitespace() {
		let mut ids = arg.split('_');
		let dataset_id: DatasetId = ids
			.next()
			.ok_or(Error::ArgumentSplitError(arg.to_owned()))?
			.parse()
			.map_err(|e| Error::ArgumentParseError(arg.to_owned(), e))?;
		let field_id: FieldId = ids
			.next()
			.ok_or(Error::ArgumentSplitError(arg.to_owned()))?
			.parse()
			.map_err(|e| Error::ArgumentParseError(arg.to_owned(), e))?;

		let fields_with_access = user
			.timeseries_with_access
			.get(&dataset_id)
			.ok_or(Error::NoAccessToDataSet(dataset_id))?;
		//prevent users requesting a field twice (this leads to an overflow later)
		if fields_with_access
			.binary_search_by(|auth| auth.as_ref().cmp(&field_id))
			.is_ok()
		{
			if let Some(field_list) = dataset_fields.get_mut(&dataset_id) {
				if !field_list.contains(&field_id) {
					field_list.push(field_id);
				}
			} else {
				dataset_fields.insert(dataset_id, vec![field_id]);
			}
		} else {
			return Err(Error::NoAccessToField(field_id));
		}
	}

	if dataset_fields.len() == 0 {
		return Err(Error::NotEnoughArguments);
	}

	let mut dataset_fields: Vec<(DatasetId, Vec<FieldId>)> = dataset_fields.drain().collect();

	//sort on datasetId to get deterministic order in the bot awnser
	dataset_fields.sort_unstable_by_key(|x| x.0);

	Ok(dataset_fields)
}

pub async fn send(
	chat_id: ChatId,
	state: &DataRouterState,
	token: &str,
	args: String,
	user: &User,
) -> Result<(), botError> {
	let mut text = String::default();
	let dataset_fields = parse_args(args, user)?;
	let datasets = &mut state.data.write().unwrap().sets;
	for (dataset_id, field_ids) in dataset_fields.iter() {
		let set = datasets.get_mut(&dataset_id).unwrap();
		let fields = &set
			.metadata
			.fields
			.iter()
			.enumerate()
			.filter(|(i, field)| field_ids.contains(&(*i as u8)))
			.map(|(_, v)| v);

		let decoder = FieldDecoder::from_fields(fields);
		let (time, values) = set
			.timeseries
			.last_line(&mut decoder)
			.map_err(|e| e.into())?;
		let time = DateTime::from_utc(chrono::NaiveDateTime::from_timestamp(time, 0), Utc);

		let set_name = &set.metadata.name;
		let time_since = format_to_duration(time);
		text.push_str(&format!(
			"dataset: {}\nlast data: {} ago\n",
			set_name, time_since
		));

		for (field, value) in fields.zip(values.into_iter()) {
			text.push_str(&format!("\t-{}:\t{:.2}\n", &field.name, value));
		}
	}

	send_text_reply(chat_id, token, text).await?;
	Ok(())
}

pub fn format_to_duration(time: DateTime<Utc>) -> String {
	let now = Utc::now();
	let duration = now.signed_duration_since(time);

	if duration.num_seconds() < 120 {
		format!("{} seconds", duration.num_seconds())
	} else if duration.num_minutes() < 120 {
		format!("{} minutes", duration.num_minutes())
	} else if duration.num_hours() < 36 {
		format!("{} hours", duration.num_hours())
	} else if duration.num_days() < 120 {
		format!("{} days", duration.num_days())
	} else {
		format!("{} weeks", duration.num_weeks())
	}
}
