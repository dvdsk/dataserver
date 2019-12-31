pub const USAGE_LIST: &str = "/alarms";
pub const USAGE_ADD: &str = "/alarm_add <alias> ... <alias>";
pub const USAGE_REMOVE: &str = "/alarm_remove <alias> ... <alias>";

pub const DESCRIPTION_LIST: &str = "show the telegram keyboard";
pub const DESCRIPTION_ADD: &str = "add aliasses to the keyboard";
pub const DESCRIPTION_REMOVE: &str = "remove aliasses from the keyboard";

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use evalexpr::{build_operator_tree, EvalexprError};
use chrono::{Weekday, self};
use regex::Regex;
use log::error;
use telegram_bot::types::refs::{ChatId, UserId};
use crate::databases::{BotUserInfo};
use crate::data_store::data_router::{DataRouterState, Alarm, AddAlarm, NotifyVia};
use crate::data_store::{DatasetId, FieldId};

use super::super::send_text_reply;
use super::super::Error as botError;


#[derive(Debug)]
pub enum Error{
    NotEnoughArguments,
	BotDatabaseError(crate::databases::UserDbError),
	NoAccessToField(FieldId),
	NoAccessToDataSet(DatasetId),
	IncorrectFieldSpecifier(String),
	NoExpression,
	IncorrectTimeUnit(String),
	ArgumentParseError(std::num::ParseIntError),
	ExpressionError(EvalexprError),
	InvalidDay(String),
}

impl From<std::num::ParseIntError> for Error {
	fn from(err: std::num::ParseIntError) -> Self {
		Error::ArgumentParseError(err)
	}
}
impl From<EvalexprError> for Error {
	fn from(err: EvalexprError) -> Self {
		Error::ExpressionError(err)
	}
}

impl Error {
	pub fn to_text(self, user_id: UserId) -> String {
		match self {
			Error::NotEnoughArguments => 
				format!("Not enough arguments, usage: {}", USAGE_LIST),
            Error::ArgumentParseError(_) => format!("One of the arguments could not be converted to a number\nuse: {}", USAGE_ADD),
            Error::NoAccessToField(field_id) => format!("You do not have access to field: {}", field_id),
            Error::NoAccessToDataSet(dataset_id) => format!("You do not have access to dataset: {}", dataset_id),
			Error::IncorrectFieldSpecifier(field) => format!("This \"{}\" is not a valid field specification, see the plotables command", field),
			Error::NoExpression => format!("An alarm must have a condition, see {}", USAGE_ADD),
			Error::IncorrectTimeUnit(unit) => format!("This \"{}\" is not a valid duration unit, options are s, m, h, d, w", unit),
			Error::ExpressionError(err) => format!("I could not understand the alarms condition. Cause: {}", err),
			Error::InvalidDay(day) => format!("I could not understand what day this is: {}", day),
			Error::BotDatabaseError(db_error) => db_error.to_text(user_id),
		}
	}
}

//example: add "3_0> 3_1 & t>9:30 & t<12:00" -d [Sunday,Saturday] -p 5h -c \plotables"
//alarm will fire on a Sunday or Saturday 
//between 9:30 and 12am if the value of 
//field 0 of dataset 3 becomes larger then field 1 of dataset 3
//the command plotables will be executed when the alarm fires
//command must be single word or enclosed in quotes
//-m message 
struct Arguments {
	expression: String,
	day: Option<HashSet<Weekday>>,
	period: Option<Duration>,
	message: Option<String>,
	command: Option<String>,
	fields: HashMap<DatasetId, Vec<FieldId>>,
}

fn parse_arguments(args: std::str::SplitWhitespace<'_>) -> Result<Arguments, Error>{	
	let args: String = args.collect();
	let exp_re = Regex::new(r#""(.*)""#).unwrap();
	let expression = exp_re.find(&args)
		.ok_or(Error::NoExpression)?
		.as_str().to_string();
	//TODO O.o timezones.... 
	//TODO rewrite time to seconds since 12am

	let day_re = Regex::new(r#"-d \[(.+)\]"#).unwrap();
	let day: Option<Result<HashSet<Weekday>,Error>> = day_re
		.find(&args)
		.map(|m| m
			.as_str()
			.split(|c| c==',' || c==' ')
			.map(|day| day.parse::<Weekday>().map_err(|_| 
				Error::InvalidDay(day.to_owned())))
			.collect()
		);//TODO O.o timezones.... 
	let day = day.transpose()?;

	let period_re = Regex::new(r#"-p \d[smhdw]"#).unwrap();
	let period = if let Some(caps) = exp_re.captures(&args){
		let numb = caps.get(1).unwrap().as_str().parse::<u64>()?;
		let unit = caps.get(2).unwrap().as_str();
		Some(match unit {
			"s" => Duration::from_secs(numb),
			"m" => Duration::from_secs(numb*60),
			"h" => Duration::from_secs(numb*60*60),
			"d" => Duration::from_secs(numb*60*60*24),
			"w" => Duration::from_secs(numb*60*60*24*7),
			_ => {return Err(Error::IncorrectTimeUnit(unit.to_owned()));},
		})
	} else {
		None
	};

	let message_re = Regex::new(r#"-m \"(.+)\""#).unwrap();
	let message = message_re
		.find(&args)
		.map(|mat| mat.as_str().to_owned());

	let command_re = Regex::new(r#"-c ([^"-\s]+|\".+\")"#).unwrap();
	let command = command_re
		.find(&args)
		.map(|mat| mat.as_str().to_owned());

	let mut fields: HashMap<DatasetId, Vec<FieldId>> = HashMap::new();
	let re = Regex::new(r#"\d+_\d+"#).unwrap();
	for ids_str in re.find_iter(&expression)
		.map(|s| s.as_str()){

		let mut split = ids_str.splitn(2, '_');
		let set_id = split.nth(0)
			.ok_or(Error::IncorrectFieldSpecifier(ids_str.to_owned()) )?
			.parse::<DatasetId>()?;
		let field_id = split.last()
			.ok_or(Error::IncorrectFieldSpecifier(ids_str.to_owned()) )?
			.parse::<FieldId>()?;
		if let Some(list) = fields.get_mut(&set_id){
			if !list.contains(&field_id){
				list.push(field_id);
			}
		} else { 
			fields.insert(set_id, vec![field_id]);
		}
	}

	Ok(Arguments {
		expression,
		day,
		period,
		message,
		command,
		fields,
	})
}

fn authorized(needed_fields: &HashMap<DatasetId, Vec<FieldId>>, 
	userinfo: &BotUserInfo) -> Result<(), Error>{

	for (set_id, list) in needed_fields.iter() {
		let fields_with_access = userinfo
			.timeseries_with_access
			.get(&set_id)
			.ok_or(Error::NoAccessToDataSet(*set_id))?;
		
		for field_id in list.iter(){
			//prevent users requesting a field twice (this leads to an overflow later)
			fields_with_access
				.binary_search_by(|auth| auth.as_ref().cmp(&field_id))
				.map_err(|_| Error::NoAccessToField(*field_id))?;
		}
	}
	Ok(())
}

pub fn add(chat_id: ChatId, token: &str, args: std::str::SplitWhitespace<'_>, 
	userinfo: BotUserInfo, state: &DataRouterState) -> Result<(), botError> {
		
	let Arguments {expression, 
		day, period, 
		message, command, 
		fields} = parse_arguments(args)?;
	authorized(&fields, &userinfo)?;

	//this tests the alarm syntax
	build_operator_tree(&expression).map_err(|e| Error::from(e))?;
	
	let tz_offset = userinfo.timezone_offset;
	let notify = NotifyVia {email: None, telegram: Some(chat_id),};
	let alarm = Alarm {
		expression,
		weekday: day,
		period: period,
		message,
		command,
		tz_offset,
		notify,
	};

	//TODO send to datarouter	
	state.data_router_addr.do_send(AddAlarm {
		alarm,
		username: userinfo.username.unwrap().clone(),
		sets: fields.keys().map(|id| *id).collect(),
	});
	
	send_text_reply(chat_id, token, "alarm is set")?;
	Ok(())
}

