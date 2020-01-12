use log::error;
use evalexpr::{self, 
	Context as evalContext,
	build_operator_tree,
	error::EvalexprError::VariableIdentifierNotFound};
use telegram_bot::types::refs::ChatId;
use chrono::{Weekday, DateTime, Utc, Datelike, Timelike};
use chrono::offset::{TimeZone, FixedOffset};
use reqwest;
use actix::prelude::*;
use threadpool::ThreadPool;
use std::time::{Duration, Instant};
use std::collections::{HashSet, HashMap};
use serde::{Serialize, Deserialize};

use crate::bot;
use crate::config::TOKEN;
use crate::data_store::DatasetId;
use super::{DataRouter, UserId, AlarmId};

#[derive(Debug)]
pub enum AlarmError {
	TooManyAlarms,
	CouldNotNotify(reqwest::Response),
}

impl From<bot::Error> for AlarmError {
	fn from(err: bot::Error) -> Self {
		match err {
			bot::Error::InvalidServerResponse(resp) => AlarmError::CouldNotNotify(resp),
			_ => unreachable!(),
		}
	}
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct NotifyVia {
	pub email: Option<String>,
	pub telegram: Option<ChatId>,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct Alarm {
	pub expression: String,
	pub inv_expr: Option<String>,
	pub weekday: Option<HashSet<Weekday>>,
	pub period: Option<Duration>,
	pub message: Option<String>,
	pub command: Option<String>,
	pub tz_offset: i32, //in hours to the east
	pub notify: NotifyVia,
}

pub struct CompiledAlarm {
	expression: evalexpr::Node,
	inv_expr: Option<evalexpr::Node>,
	inverted: bool,

	expr_string: String,
	weekday: Option<HashSet<Weekday>>,
	period: Option<(Duration, Instant)>,
	message: Option<String>,
	command: Option<String>,
	timezone: FixedOffset, //in hours to the east
	notify: NotifyVia,
}

impl From<Alarm> for CompiledAlarm {
	fn from(alarm: Alarm) -> Self {
		let Alarm {expression, 
			inv_expr, weekday, 
			period, message, command, 
			tz_offset, notify} = alarm;
		let timezone = FixedOffset::east(tz_offset*3600); 
		let expr_string = expression;
		let expression = build_operator_tree(&expr_string).unwrap();
		let inv_expr = inv_expr.map(|expr| build_operator_tree(&expr).unwrap());
		let period = period.map(|delay| (delay, Instant::now()));

		CompiledAlarm {
			expression,
			inv_expr,
			inverted: false,
			expr_string,
			weekday,
			period,
			message,
			command,
			timezone,
			notify,
		}
	}
}

impl CompiledAlarm {
	pub fn evalute(&mut self, context: &mut evalexpr::HashMapContext, 
		now: &DateTime::<Utc>, pool: &ThreadPool) -> Result<(), AlarmError> {
		
		dbg!();
		if let Some((period, last)) = self.period {
			if last.elapsed() < period {return Ok(());}
		}
		
		let now_user_tz = self.timezone.from_utc_datetime(&now.naive_utc());
		let today_user_tz = now_user_tz.weekday();
		if let Some(active_weekdays) = &self.weekday {
			if !active_weekdays.contains(&today_user_tz){return Ok(());}
		}

		let seconds_since_midnight = now_user_tz.num_seconds_from_midnight() as f64;
		context.set_value("t".to_string(), seconds_since_midnight.into()).unwrap();
		
		let to_evaluate = if self.inverted { 
			self.inv_expr.as_ref().unwrap() 
		} else { 
			&self.expression
		};

		dbg!(&to_evaluate);
		dbg!(&context);

		match to_evaluate.eval_boolean_with_context(context){
			Ok(alarm_condition) => if alarm_condition {
				let notify = self.notify.clone();
				let message = self.message.clone();
				let command = self.command.clone();
				let expr_string = self.expr_string.clone();
				let inverted = self.inverted;
				pool.execute(move || {
					sound_alarm(notify, message, 
						expr_string, command, inverted);
				});
				if self.inv_expr.is_some() {self.inverted = !self.inverted;}
			},
			Err(error) => match error {
					VariableIdentifierNotFound(_) => {
						dbg!();
						//TODO if happens for long time warn user
					}
					_ => {error!("{:?}", error); dbg!();},
			}
		}
		Ok(())
	}
}

//TODO //FIXME has to handle error without returning
fn sound_alarm(notify: NotifyVia, message: Option<String>,
	expression: String, command: Option<String>, inverted: bool){

	let to_send = if let Some(message) = &message {
		message.to_owned()
	} else {
		if inverted {
			format!("alarm re-enabled: {}", expression)
		} else {
			format!("alarm fired: {}", expression)
		}
	};

	dbg!();
	if let Some(_email) = &notify.email {
		todo!();
	}
	if let Some(chat_id) = &notify.telegram {
		bot::send_text_reply_blocking(*chat_id, TOKEN, to_send);

		if let Some(command) = &command {
			todo!();
			//let user_id = ;
			//let state = ;
			//bot::handle_command(command, chat_id, user_id, state);
		}
	}
}

#[derive(Message)]
#[rtype(result = "Result<(),AlarmError>")]
pub struct AddAlarm {
    pub alarm: Alarm,
	pub user_id: UserId,
	pub alarm_id: AlarmId,
    pub sets: Vec<DatasetId>,
}

impl Handler<AddAlarm> for DataRouter {
	type Result = Result<(),AlarmError>;

	fn handle(&mut self, msg: AddAlarm, _: &mut Context<Self>) -> Self::Result {
		for set_id in msg.sets {
			let list = if let Some(list) = self.alarms_by_set.get_mut(&set_id){
				list
			} else {
				self.alarms_by_set.insert(set_id, HashMap::new());
				self.alarms_by_set.get_mut(&set_id).unwrap()
			};
			let alarm: CompiledAlarm = msg.alarm.clone().into();
			list.insert((msg.user_id, msg.alarm_id), alarm);
        }
		Ok(())
	}
}

#[derive(Message)]
#[rtype(result = "")]
pub struct RemoveAlarm {
	sets: Vec<DatasetId>,
	user_id: UserId,
	alarm_id: AlarmId, 
}

impl Handler<RemoveAlarm> for DataRouter {
	type Result = ();

	fn handle(&mut self, msg: RemoveAlarm, _: &mut Context<Self>) -> Self::Result {
		for set in msg.sets {
			if let Some(alarms) = self.alarms_by_set.get_mut(&set){
				alarms.remove(&(msg.user_id,msg.alarm_id));
			}
		}
	}
}