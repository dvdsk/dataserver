pub const USAGE: &str = "/show <plotable_id 1> ... <plotable_id n>";
pub const DESCRIPTION: &str = "sends the current value(s) of the requested plotable(s)";

use std::collections::HashMap;
use chrono::{DateTime, Utc};

use telegram_bot::types::refs::{ChatId, UserId};

use crate::data_store::{DatasetId, FieldId};
use crate::databases::{BotUserInfo, UserDbError};
use crate::data_store::data_router::DataRouterState;

use super::super::send_text_reply;
use super::super::Error as botError;

#[derive(Debug)]
pub enum Error{
    ArgumentParseError(String, std::num::ParseIntError),
    ArgumentSplitError(String),
    NoAccessToField(FieldId),
    NoAccessToDataSet(DatasetId),
    NoData(DatasetId),
	BotDatabaseError(UserDbError),
    NotEnoughArguments,
}

impl Error {
    pub fn to_text(self, user_id: UserId) -> String {
        match self {
            Error::ArgumentParseError(arg, _err) => format!("The argument {} could not be parsed", arg),
            Error::ArgumentSplitError(arg) => format!("The argument {} could not be interperted, does it contain a \":\"?", arg),
            Error::NoAccessToField(field_id) => format!("You do not have access to field: {}", field_id),
            Error::NoAccessToDataSet(dataset_id) => format!("You do not have access to dataset: {}", dataset_id),
            Error::NoData(set_id) => format!("There is no data for dataset: {}", set_id),
            Error::BotDatabaseError(db_error) => db_error.to_text(user_id),
            Error::NotEnoughArguments => format!("Not enough arguments\nuse: {}", USAGE),            
        }
    }
}

impl From<UserDbError> for Error {
	fn from(error: UserDbError) -> Self {
		Error::BotDatabaseError(error)
	}
}

fn parse_args(args: std::str::SplitWhitespace<'_>, userinfo: &BotUserInfo)
    -> Result<Vec<(DatasetId, Vec<FieldId>)>, Error> {
       //keep a list of fields for each dataset
    let mut dataset_fields: HashMap<DatasetId, Vec<FieldId>> = HashMap::new();

    for arg in args {
        let mut ids = arg.split('_');       
        let dataset_id: DatasetId = ids.next()
            .ok_or(Error::ArgumentSplitError(arg.to_owned()))?
            .parse().map_err(|e| Error::ArgumentParseError(arg.to_owned(), e))?;
        let field_id: FieldId = ids.next()
            .ok_or(Error::ArgumentSplitError(arg.to_owned()))?
            .parse().map_err(|e| Error::ArgumentParseError(arg.to_owned(), e))?;

        let fields_with_access = userinfo
            .timeseries_with_access
            .get(&dataset_id)
            .ok_or(Error::NoAccessToDataSet(dataset_id))?;
            //prevent users requesting a field twice (this leads to an overflow later)
        if fields_with_access.binary_search_by(|auth| auth.as_ref().cmp(&field_id)).is_ok() {
            
            if let Some(field_list) = dataset_fields.get_mut(&dataset_id){
                if !field_list.contains(&field_id){
                    field_list.push(field_id);
                }
            } else {
                dataset_fields.insert(dataset_id, vec!(field_id));
            }
        } else { 
            return Err(Error::NoAccessToField(field_id));
        }
    }
    
    if dataset_fields.len() == 0 {
        return Err(Error::NotEnoughArguments);
    }

    let mut dataset_fields: Vec<(DatasetId, Vec<FieldId>)> = dataset_fields
        .drain()
        .collect();

    //sort on datasetId to get deterministic order in the bot awnser
    dataset_fields.sort_unstable_by_key(|x| x.0); 

    Ok(dataset_fields)
}

pub fn send(chat_id: ChatId, state: &DataRouterState, token: &str, 
    args: std::str::SplitWhitespace<'_>, userinfo: &BotUserInfo)
     -> Result<(), botError>{

    let mut text = String::default();
    let dataset_fields = parse_args(args, userinfo)?;
    let datasets = &mut state.data.write().unwrap().sets;
    for (dataset_id, field_ids) in dataset_fields.iter(){
        let set = datasets.get_mut(&dataset_id).unwrap();
        let (time, line) = set.timeseries.decode_last_line().map_err(|_| Error::NoData(*dataset_id))?; //TODO make this return Option(tuple)
        let fields = &set.metadata.fields;
        let set_name = &set.metadata.name;
        
        let time_since = format_to_duration(time);
        text.push_str(&format!("dataset: {}\nlast data: {} ago\n", set_name, time_since));

        for field in field_ids.iter().map(|id| &fields[*id as usize]){
            let value: f32 = field.decode(&line);
            let field_name = &field.name;

            text.push_str(&format!("\t-{}:\t{:.2}\n",field_name, value));
        }
    }
    
    send_text_reply(chat_id, token, text)?;
    Ok(())
}

fn format_to_duration(time: DateTime<Utc>) -> String {
    
    let now = Utc::now();
    let duration = now.signed_duration_since(time);
    
    if duration.num_seconds() < 120 {
        format!("{} seconds",duration.num_seconds())
    } else if duration.num_minutes() < 120 {
        format!("{} minutes",duration.num_minutes())  
    } else if duration.num_hours() < 36 {
        format!("{} hours",duration.num_hours())
    } else if duration.num_days() < 120 {
        format!("{} days",duration.num_days())
    } else {
        format!("{} weeks",duration.num_weeks())
    }
}