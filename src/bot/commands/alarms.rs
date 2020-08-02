pub const USAGE: &str = "/alarm";
pub const DESCRIPTION: &str = "list, add and remove sensor notifications";

pub const HELP_ADD: &'static str = 
	"/alarm add [options] \"condition\"\n\
	example: add \"3_0> 3_1 & t>9:30 & t<12:00\" -d [Sunday,Saturday] -p 5h -c /plotables\
	\ncondition; a boolean statement that may use these binairy operators: \
	^ * / % + - < > == != && || on sensor fields (see /plotables for options) \
	or time (use the symbole \"t\")\
	\npossible options:\n\
	-d [Weekday, .... , Weekday]\n\
	days on which the condition should be evaluated \
	example: -d [Saturday, Sunday]\n\
	-p <number><unit>\n\
	override the default minimal time between \
	activation of alarm. The time unit can be \
	s,m,h,d or w\n\
	-c <command>\n\
	here command should be a valid telegram command \
	for this bot. If the command is more then one word \
	long it should be enclosed in quotes\n\
	-i <percentage>\n\
	prevent alarm from being triggerd continuesly, once \
	an alarm is triggerd disarm and set an inverse \
	alarm that will re-enable it once one of the values \
	it watches deviates the given percentage from the alarm \
	activation value\n";
pub const HELP_LIST: &'static str = 
	"/alarm list\n\
	shows for all set alarms their: id, condition, timezone and action\
	performed when the condition is satisfied.\n";
pub const HELP_REMOVE: &'static str = 
	"/alarm remove list_numb_1 list_numb_2....list_numb_n\n\
	removes one or multiple active alarms, space seperate list \
	of the numbers in front of the expressions of the alarm list.";

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use evalexpr::{build_operator_tree, EvalexprError};
use chrono::{Weekday, self};
use regex::{Regex, Captures};
use log::error;
use telegram_bot::types::refs::ChatId;
use telegram_bot::types::refs::UserId as TelegramUserId;
use crate::databases::{User, AlarmDbError};
use crate::data_store::data_router::{DataRouterState, Alarm, NotifyVia};
use crate::data_store::data_router::{AddAlarm, RemoveAlarm};
use crate::data_store::{DatasetId};
use bitspec::FieldId;

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
	InvalidSubCommand(String),
	TooManyAlarms,
	NotAnAlarmNumber(String),
	DbError(AlarmDbError),
}

impl From<AlarmDbError> for Error {
	fn from(err: AlarmDbError) -> Self {
		Error::DbError(err)
	}
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
	pub fn to_text(self, user_id: TelegramUserId) -> String {
		match self {
			Error::NotEnoughArguments => format!("Not enough arguments, usage: {}", HELP_ADD),
            Error::ArgumentParseError(_) => format!("One of the arguments could not be converted to a number\nuse: {}", HELP_ADD),
            Error::NoAccessToField(field_id) => format!("You do not have access to field: {}", field_id),
            Error::NoAccessToDataSet(dataset_id) => format!("You do not have access to dataset: {}", dataset_id),
			Error::IncorrectFieldSpecifier(field) => format!("This \"{}\" is not a valid field specification, see the plotables command", field),
			Error::NoExpression => format!("An alarm must have a condition, type /alarms for help"),
			Error::IncorrectTimeUnit(unit) => format!("This \"{}\" is not a valid duration unit, options are s, m, h, d, w", unit),
			Error::ExpressionError(err) => format!("I could not understand the alarms condition. Cause: {}", err),
			Error::InvalidDay(day) => format!("I could not understand what day this is: {}", day),
			Error::BotDatabaseError(db_err) => db_err.to_text(user_id),
			Error::InvalidSubCommand(input) => format!("not a sub command for alarms: {}", input),
			Error::TooManyAlarms => String::from("can not set more then 255 alarms"),//FIXME not true after readme
			Error::DbError(db_err) => db_err.to_text(),
			Error::NotAnAlarmNumber(input) => format!("Could not recognise an alarm number in: {}", input),
		}
	}
}

//alarm will fire on a Sunday or Saturday 
//between 9:30 and 12am if the value of 
//field 0 of dataset 3 becomes larger then field 1 of dataset 3
//the command plotables will be executed when the alarm fires
//command must be single word or enclosed in quotes
//-m message 
struct Arguments {
	expression: String,
	counter_expr: bool,
	day: Option<HashSet<Weekday>>,
	period: Option<Duration>,
	message: Option<String>,
	command: Option<String>,
	fields: HashMap<DatasetId, Vec<FieldId>>,
}

/// replaces any occurence of dd:dd (for example 22:15) 
/// to the number of seconds since 00:00
fn format_time_to_seconds(expression: String) -> String {
	let time_re = Regex::new(r#"(\d{1,2}):(\d\d)"#).unwrap();
	time_re.replacen(&expression, 0, |caps: &Captures| {
			let hours: u32 = caps
				.get(1).unwrap()
				.as_str().parse().unwrap();
			let minutes: u32 = caps
				.get(2).unwrap()
				.as_str().parse().unwrap();
			format!("{}",minutes*60+hours*3600)
	}).to_string()
}

/// replaces any occurence of t < numb (or larger or equal) 
/// by the human readable format hh:mm since midnight
pub fn format_time_human_readable(expression: String) -> String {
	let time_re = Regex::new(r#"t\s?(<|>|>=|<=)\s?(\d+)"#).unwrap();
	time_re.replacen(&expression, 0, |caps: &Captures| {
			let equality = caps
				.get(1).unwrap().as_str();
			let seconds: u32 = caps
				.get(2).unwrap()
				.as_str().parse().unwrap();
			let hours = seconds/3600;
			let minutes = (seconds/60) % 60;
			dbg!(seconds); dbg!(hours); dbg!(minutes);

			if minutes < 10 {
				if hours < 10 {
					format!("t {} 0{}:0{}", equality, hours, minutes)
				} else {
					format!("t {} {}:0{}", equality, hours, minutes)
				}
			} else {
				if hours < 10 {
					format!("t {} 0{}:{}", equality, hours, minutes)
				} else {
					format!("t {} {}:{}", equality, hours, minutes)
				}
			}
	}).to_string()
}

fn parse_arguments(args: &str) -> Result<Arguments, Error>{	
	dbg!(&args);//FIXME problem seems to be no spaces anymore here
	let exp_re = Regex::new(r#""(.*)""#).unwrap();
	let expression = exp_re.find(&args)
		.ok_or(Error::NoExpression)?.as_str();
	let expression = expression.get(1..expression.len()-1).unwrap().to_owned();
	dbg!("exp");
	dbg!(&expression);
	let expression = format_time_to_seconds(expression);

	let day_re = Regex::new(r#"-d \[(.+)\]"#).unwrap();
	let day: Option<Result<HashSet<Weekday>,Error>> = day_re
		.captures(&args)
		//.map(|f| f.get(1))
		//.ok_or(Error::InvalidDay(args.to_owned()))?
		.map(|m| m.get(1).unwrap()
			.as_str()
			.split(|c| c==',' || c==' ')
			.filter(|s| s.len()>5)
			.map(|day| {dbg!(&day); day
				.parse::<Weekday>()
				.map_err(|_| Error::InvalidDay(day.to_owned()))}
			)
			.collect()
		);
	let day = day.transpose()?;

	let period_re = Regex::new(r#"-p (\d+)([smhdw])"#).unwrap();
	let period = if let Some(caps) = period_re.captures(&args){
		let numb = caps.get(1).unwrap().as_str().parse::<u64>()?;
		let unit = caps.get(2).unwrap().as_str();
		if numb == 0 {
			None 
		} else {
			Some(match unit {
				"s" => Duration::from_secs(numb),
				"m" => Duration::from_secs(numb*60),
				"h" => Duration::from_secs(numb*60*60),
				"d" => Duration::from_secs(numb*60*60*24),
				"w" => Duration::from_secs(numb*60*60*24*7),
				_ => {return Err(Error::IncorrectTimeUnit(unit.to_owned()));},
			})
		}
	} else {
		Some(Duration::from_secs(1*60*60))
	};

	let message_re = Regex::new(r#"-m "(.+)""#).unwrap();
	let message = message_re
		.find(&args)
		.map(|mat| mat.as_str().to_owned());

	let command_re = Regex::new(r#"-c ([^"\-\s]+|".+")"#).unwrap();
	let command = command_re
		.find(&args)
		.map(|mat| mat.as_str().to_owned());

	let counter_expr = args.contains("-bc");

	let mut fields: HashMap<DatasetId, Vec<FieldId>> = HashMap::new();
	let re = Regex::new(r#"\d+_\d+"#).unwrap();
	for ids_str in re.find_iter(&expression)
		.map(|s| s.as_str()){
		dbg!(&ids_str);

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
		counter_expr,
		day,
		period,
		message,
		command,
		fields,
	})
}

fn authorized(needed_fields: &HashMap<DatasetId, Vec<FieldId>>, 
	user: &User) -> Result<(), Error>{

	for (set_id, list) in needed_fields.iter() {
		let fields_with_access = user
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

async fn add(chat_id: ChatId, token: &str, args: &str, 
	user: User, state: &DataRouterState) -> Result<(), botError> {
		
	let Arguments {expression, counter_expr,
		day, period, message, command, 
		fields} = parse_arguments(args)?;
	authorized(&fields, &user)?;
	dbg!();

	//this tests the alarm syntax
	build_operator_tree(&expression).map_err(|e| {
		error!("could not build operator tree: {}", e);
		Error::from(e)
	})?;
	dbg!();
	let tz_offset = user.timezone_offset;
	let notify = NotifyVia {email: None, telegram: Some(chat_id),};
	let inv_expr = if counter_expr {
		Some(get_inverse_expression(&expression, 0.05))
	} else { None };
	dbg!();
	let alarm = Alarm {
		expression,
		inv_expr,
		weekday: day,
		period: period,
		message,
		command,
		tz_offset,
		notify,
	};
	dbg!();
	let alarm_id = state.alarm_db.add(&alarm, user.id).map_err(|e| Error::from(e))?;
	state.data_router_addr.send(AddAlarm {
		alarm,
		user_id: user.id,
		alarm_id,
		sets: fields.keys().map(|id| *id).collect(),
	}).await.unwrap().map_err(|_| Error::TooManyAlarms)?;
	dbg!();
	send_text_reply(chat_id, token, "alarm is set").await?;
	Ok(())
}

pub async fn handle(chat_id: ChatId, token: &str, text: String, 
	user: User, state: &DataRouterState) -> Result<(), botError> {

	let mut text = text.trim_start().splitn(2, ' ');
	let subcommand = text.next().unwrap_or_default();
	let args = text.next().unwrap_or_default();

	match subcommand {
		"add" => add(chat_id, token, args, user, state).await,
		"list" => list(chat_id, token, user, state).await,
		"remove" => remove(chat_id, token, args, user, state).await,
		_ => send_text_reply(chat_id, token, format!("Could not recognise the \
			subcommand, see documentation: \n{}\n{}\n{}", 
			HELP_LIST, HELP_ADD, HELP_REMOVE)).await,
	}
}

async fn list(chat_id: ChatId, token: &str, user: User, state: &DataRouterState)
 -> Result<(), botError> {
	
	let entries = state.alarm_db.list_users_alarms(user.id);
	if entries.is_empty() {
		send_text_reply(chat_id, token, "I have no alarms for you").await?;
		return Ok(())
	}

	let mut list = String::default();
	for (counter, alarm) in entries {
		list.push_str(&format!("{}\texpr: {}\n", 
			counter, 
			format_time_human_readable(alarm.expression))
		);
			
		if let Some(days) = alarm.weekday {
			let valid_days: String = days.iter()
				.map(|d| format!("{:?},", d)) //FIXME debug should move to display
				.collect();
			list.push_str(&format!("valid on: [{}]\n", valid_days));
		}

		if let Some(period) = alarm.period {
			list.push_str(&format!("cooldown: {:?}\n", period)); //FIXME custom format funct
		}

		if let Some(message) = alarm.message {
			list.push_str(&format!("message: {}\n", message));
		}

		if let Some(inv) = alarm.inv_expr {
			list.push_str(&format!("reactivating if: {}\n", 
			format_time_human_readable(inv)));
		}
	}
	dbg!(&list);
	send_text_reply(chat_id, token, list).await?;
	Ok(())
}

async fn remove(chat_id: ChatId, token: &str, args: &str, user: User, 
	state: &DataRouterState) -> Result<(), botError> {

	let numbs: Result<Vec<usize>,_> = args.split_whitespace()
		.map(|s| s
			.parse()
			.map_err(|_| Error::NotAnAlarmNumber(s.to_owned()))
		).collect();
	let mut numbs = numbs?;
	numbs.sort_unstable();

	for numb in numbs.iter().rev() {
		let (alarm, alarm_id) = state.alarm_db
			.remove(user.id, *numb)
			.map_err(|e| Error::from(e))?;
		let sets = alarm.watched_sets();

		state.data_router_addr.send(RemoveAlarm {
			sets,
			user_id: user.id,
			alarm_id,
		}).await.unwrap();
	}

	if numbs.len() > 1 {
		send_text_reply(chat_id, token, "alarms removed").await?;
	} else {
		send_text_reply(chat_id, token, "alarm removed").await?;
	}
	Ok(())
}

//TODO invert all AND and OR operators
fn get_inverse_expression(expression: &str, percentage: f32) -> String {
	let var_vs_numb = Regex::new(
		r#"(\d+_\d+)\s((?:<=)|(?:>=)|(?:==)|(?:!=)|>|<)\s(\d+\.?\d+*)"#)
		.unwrap();
	let numb_vs_var = Regex::new(
		r#"(\d+\.?\d*)\s((?:<=)|(?:>=)|(?:==)|(?:!=)|>|<)\s(\d+_\d+)"#)
		.unwrap();

	let adjust_up = |x| x*(1f32+percentage);
	let adjust_down = |x| x*(1f32-percentage);
	let inverse = var_vs_numb.replace_all(expression, |caps: &Captures| {
		let numb = caps[3].parse::<f32>().unwrap();
		let (inv_op, adj_numb) = match &caps[2] {
			"<=" => (">", adjust_up(numb)),
			"<" => (">=", adjust_up(numb)),
			">" => ("<=", adjust_down(numb)),
			">=" => ("<", adjust_down(numb)),
			"==" => ("!=", numb),
			"!=" => ("==", numb),
			_ => unreachable!(),
		};
		format!("{} {} {}", &caps[1], inv_op, adj_numb)
	});
	let inverse = numb_vs_var.replace_all(&inverse, |caps: &Captures| {
		let numb = caps[1].parse::<f32>().unwrap();
		let (inv_op, adj_numb) = match &caps[2] {
			"<=" => (">", adjust_up(numb)),
			"<" => (">=", adjust_up(numb)),
			">" => ("<=", adjust_down(numb)),
			">=" => ("<", adjust_down(numb)),
			"==" => ("!=", numb),
			"!=" => ("==", numb),
			_ => unreachable!(),
		};
		format!("{} {} {}", adj_numb, inv_op, &caps[3])
	});

	inverse.to_string()
}

#[cfg(test)]
mod tests {
	use super::*;
	
	#[test]
	fn test_invert() {
		let inverse = get_inverse_expression("13_11 > 21.53", 0.1);
		let correct_inv = "13_11 <= 19.377";
		assert_eq!(inverse, correct_inv);

		let inverse = get_inverse_expression("1_1 >= 2", 0.1);
		let correct_inv = "1_1 < 1.8";
		assert_eq!(inverse, correct_inv);

		let inverse = get_inverse_expression("10_0 < 5", 0.1);
		let correct_inv = "10_0 >= 5.5";
		assert_eq!(inverse, correct_inv);

		let inverse = get_inverse_expression("5_0 != 5", 0.1);
		let correct_inv = "5_0 == 5";
		assert_eq!(inverse, correct_inv);

		let inverse = get_inverse_expression("5 != 1_2", 0.1);
		let correct_inv = "5 == 1_2";
		assert_eq!(inverse, correct_inv);
	}

	#[test]
	fn test_rewrite_time() {
		let tests = vec!(
			"3_1 < 5.3 && t<09:34",
			"3_1 < 5.3 && t < 09:34",
			"3_1 < 5.3 && t< 23:00",
			"3_1 < 5.3 && t< 10:05",
			"3_1 < 5.3 && t< 01:09",
			"3_1 < 5.3 && t< 19:51",
			"3_1 < 5.3 && t< 10:03",
		);

		for test_expr in tests{
			let rewritten = format_time_to_seconds(test_expr.to_owned());
			let mut formatted = format_time_human_readable(rewritten);

			let mut test_expr = test_expr.to_owned();
			test_expr.retain(|c| c != ' ');
			formatted.retain(|c| c != ' ');
			
			assert_eq!(test_expr, formatted);
		}
	}
}