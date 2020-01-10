pub const USAGE: &str = "/alarms";
pub const DESCRIPTION: &str = "list, add and remove sensor notifications";

pub const HELP_ADD: &'static str = 
	"add [options] \"condition\"\n\
	example: add \"3_0> 3_1 & t>9:30 & t<12:00\" -d [Sunday,Saturday] -p 5h -c \\plotables\
	\ncondition; a boolean statement that may use these binairy operators: \
	^ * / % + - < > == != && || on sensor fields (see \\plotables for options) \
	or time (use the symbole \"t\")\
	\npossible options:\n\
	-d [Weekday, .... , Weekday]\n\
	days on which the condition should be evaluated \
	example: -d [Saturday, Sunday]\n\
	-p <number><unit>\n\
	minimal time between activation of alarm here \
	unit can be s,m,h,d or w\n\
	-c <command>\n\
	here command should be a valid telegram command \
	for this bot. If the command is more then one word \
	long it should be enclosed in quotes\n\
	-bc\n\
	prevent alarm from being triggerd continuesly, once \
	an alarm is triggerd disarm and set a counter \
	alarm that will re-enable it once one of the values \
	it watches deviates 10% from the alarm activation value\n";
pub const HELP_LIST: &'static str = 
	"list\n\
	shows for all set alarms their: id, condition, timezone and action\
	performed when the condition is satisfied.\n";
pub const HELP_REMOVE: &'static str = 
	"remove [alarm_id]\n\
	removes an active alarm";

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use evalexpr::{build_operator_tree, EvalexprError};
use chrono::{Weekday, self};
use regex::{Regex, Captures};
use telegram_bot::types::refs::{ChatId, UserId};
use crate::databases::User;
use crate::data_store::data_router::{DataRouterState, Alarm, NotifyVia};
use crate::data_store::data_router::{AddAlarm, ListAlarms, AlarmId};
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
	InvalidSubCommand(String),
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
			Error::NotEnoughArguments => format!("Not enough arguments, usage: {}", HELP_ADD),
            Error::ArgumentParseError(_) => format!("One of the arguments could not be converted to a number\nuse: {}", HELP_ADD),
            Error::NoAccessToField(field_id) => format!("You do not have access to field: {}", field_id),
            Error::NoAccessToDataSet(dataset_id) => format!("You do not have access to dataset: {}", dataset_id),
			Error::IncorrectFieldSpecifier(field) => format!("This \"{}\" is not a valid field specification, see the plotables command", field),
			Error::NoExpression => format!("An alarm must have a condition, type /alarms for help"),
			Error::IncorrectTimeUnit(unit) => format!("This \"{}\" is not a valid duration unit, options are s, m, h, d, w", unit),
			Error::ExpressionError(err) => format!("I could not understand the alarms condition. Cause: {}", err),
			Error::InvalidDay(day) => format!("I could not understand what day this is: {}", day),
			Error::BotDatabaseError(db_error) => db_error.to_text(user_id),
			Error::InvalidSubCommand(input) => format!("not a sub command for alarms: {}", input),
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
fn rewrite_time(expression: String) -> String {
	let time_re = Regex::new(r#"(\d{1,2}):(\d\d)"#).unwrap();
	time_re.replacen(&expression, 0, |caps: &Captures| {
			let minutes: u32 = caps
				.get(1).unwrap()
				.as_str().parse().unwrap();
			let hours: u32 = caps
				.get(2).unwrap()
				.as_str().parse().unwrap();
			format!("{}",minutes*60+hours*3600)
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
	let expression = rewrite_time(expression);

	let day_re = Regex::new(r#"-d \[(.+)\]"#).unwrap();
	let day: Option<Result<HashSet<Weekday>,Error>> = day_re
		.find(&args)
		.map(|m| m
			.as_str()
			.split(|c| c==',' || c==' ')
			.map(|day| day.parse::<Weekday>().map_err(|_| 
				Error::InvalidDay(day.to_owned())))
			.collect()
		);
	let day = day.transpose()?;

	let period_re = Regex::new(r#"-p \d[smhdw]"#).unwrap();
	let period = if let Some(caps) = period_re.captures(&args){
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
	dbg!(&expression);
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

	//this tests the alarm syntax
	build_operator_tree(&expression).map_err(|e| Error::from(e))?;
	
	let tz_offset = user.timezone_offset;
	let notify = NotifyVia {email: None, telegram: Some(chat_id),};
	let inv_expr = if counter_expr {
		Some(get_inverse_expression(&expression, 0.1))
	} else { None };

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

	dbg!(&fields);
	let res = state.data_router_addr.send(AddAlarm {
		alarm,
		username: user.name.clone(),
		sets: fields.keys().map(|id| *id).collect(),
	}).await.unwrap();
	dbg!(res);
	send_text_reply(chat_id, token, "alarm is set").await?;
	Ok(())
}

pub async fn handle(chat_id: ChatId, token: &str, text: String, 
	user: User, state: &DataRouterState) -> Result<(), botError> {

	dbg!(&text);
	let mut text = text.trim_start().splitn(2, ' ');
	dbg!(&text);
	let subcommand = text.next().unwrap_or_default();
	let args = text.next().unwrap_or_default();
	dbg!(subcommand);
	dbg!(args);
	match subcommand {
		"add" => add(chat_id, token, args, user, state).await,
		"list" => list(chat_id, token, user, state).await,
		_ => send_text_reply(chat_id, token, format!("Could not recognise the \
			subcommand, see documentation: \n{}\n{}\n{}", 
			HELP_LIST, HELP_ADD, HELP_REMOVE)).await,
	}
}

/*pub struct Alarm {
	pub expression: String,
	pub weekday: Option<HashSet<Weekday>>,
	pub period: Option<Duration>,
	pub message: Option<String>,
	pub command: Option<String>,
	pub tz_offset: i32, //in hours to the east
	pub notify: NotifyVia,
}*/

async fn list(chat_id: ChatId, token: &str, user: User, state: &DataRouterState)
 -> Result<(), botError> {
	dbg!("");
	let alarms: Option<Vec<(DatasetId, AlarmId, Alarm)>> 
	= state.data_router_addr.send(ListAlarms {
		username: user.name,
	}).await.unwrap();
	dbg!();
	let mut list = String::default();
	if let Some(alarms) = alarms {
		dbg!(&alarms);
		for (set_id, alarm_id, alarm) in alarms {
			dbg!(&alarm_id);
			list.push_str(&format!("{}_{}\n\texpr: {}", 
				set_id, alarm_id, 
				alarm.expression));
			
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
		}
	} else {
		list.push_str("I have no alarms");
	}
	dbg!(&list);
	send_text_reply(chat_id, token, list).await?;
	Ok(())
}

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
}