use actix::prelude::*;
use chrono::offset::{FixedOffset, TimeZone};
use chrono::{DateTime, Datelike, Timelike, Utc, Weekday};
use evalexpr::{
	self, build_operator_tree, error::EvalexprError::VariableIdentifierNotFound,
	Context as evalContext,
};
use log::{error, warn};
use regex::Regex;
use reqwest;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use telegram_bot::types::refs::ChatId;
use threadpool::ThreadPool;

use super::{AlarmId, DataRouter, UserId};
use crate::bot;
use crate::data_store::DatasetId;

#[derive(Debug)]
pub enum AlarmError {
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

impl Alarm {
	///expression needs to be valid or this will panic
	pub fn watched_sets(&self) -> Vec<DatasetId> {
		let re: regex::Regex = Regex::new(r#"(\d+)_\d+"#).unwrap();
		let sets = re
			.captures_iter(&self.expression)
			.map(|caps| caps[1].parse().unwrap())
			.collect();
		sets
	}
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
		let Alarm {
			expression,
			inv_expr,
			weekday,
			period,
			message,
			command,
			tz_offset,
			notify,
		} = alarm;
		let timezone = FixedOffset::east(tz_offset * 3600);
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
	pub fn evalute(
		&mut self,
		context: &mut evalexpr::HashMapContext,
		now: &DateTime<Utc>,
		pool: &ThreadPool,
		token: String,
	) -> Result<(), AlarmError> {
		if let Some((period, last)) = self.period {
			if last.elapsed() < period {
				return Ok(());
			}
		}

		let now_user_tz = self.timezone.from_utc_datetime(&now.naive_utc());
		let today_user_tz = now_user_tz.weekday();
		if let Some(active_weekdays) = &self.weekday {
			if !active_weekdays.contains(&today_user_tz) {
				return Ok(());
			}
		}

		let seconds_since_midnight = now_user_tz.num_seconds_from_midnight() as f64;
		context
			.set_value("t".to_string(), seconds_since_midnight.into())
			.unwrap();

		let to_evaluate = if self.inverted {
			self.inv_expr.as_ref().unwrap()
		} else {
			&self.expression
		};

		match to_evaluate.eval_boolean_with_context(context) {
			Ok(alarm_condition) => {
				if alarm_condition {
					let notify = self.notify.clone();
					let message = self.message.clone();
					let command = self.command.clone();
					let expr_string = self.expr_string.clone();
					let inverted = self.inverted;
					pool.execute(move || {
						sound_alarm(notify, message, expr_string, command, inverted, token);
					});
					if self.inv_expr.is_some() {
						self.inverted = !self.inverted;
					}
				}
			}
			Err(error) => match error {
				VariableIdentifierNotFound(_) => {
					warn!("variable not found, normal shortly after startup");
					//TODO if happens for long time warn user
				}
				_ => {
					error!("{:?}", error);
					dbg!();
				}
			},
		}
		Ok(())
	}
}

//TODO //FIXME has to handle error without returning
fn sound_alarm(
	notify: NotifyVia,
	message: Option<String>,
	expression: String,
	command: Option<String>,
	inverted: bool,
	token: String,
) {
	let to_send = if let Some(message) = &message {
		message.to_owned()
	} else {
		if inverted {
			format!(
				"alarm re-enabled: {}",
				bot::alarms::format_time_human_readable(expression)
			)
		} else {
			format!(
				"alarm fired: {}",
				bot::alarms::format_time_human_readable(expression)
			)
		}
	};

	if let Some(_email) = &notify.email {
		todo!();
	}
	if let Some(chat_id) = &notify.telegram {
		if let Err(err) = bot::send_text_reply_blocking(*chat_id, &token, to_send) {
			error!("could not notify client via telegram: {:?}", err);
		}

		if let Some(_command) = &command {
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
	type Result = Result<(), AlarmError>;

	fn handle(&mut self, msg: AddAlarm, _: &mut Context<Self>) -> Self::Result {
		for set_id in msg.sets {
			let list = if let Some(list) = self.alarms_by_set.get_mut(&set_id) {
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
	pub sets: Vec<DatasetId>,
	pub user_id: UserId,
	pub alarm_id: AlarmId,
}

impl Handler<RemoveAlarm> for DataRouter {
	type Result = ();

	fn handle(&mut self, msg: RemoveAlarm, _: &mut Context<Self>) -> Self::Result {
		for set in msg.sets {
			if let Some(alarms) = self.alarms_by_set.get_mut(&set) {
				alarms.remove(&(msg.user_id, msg.alarm_id));
			}
		}
	}
}
