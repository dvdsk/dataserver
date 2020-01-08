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

use std::time::{Duration, Instant};
use std::collections::{HashSet, HashMap};

use crate::bot;
use crate::config::TOKEN;
use crate::data_store::DatasetId;
use super::{DataRouter};

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

pub type Id = u8;
#[derive(Clone)]
pub struct NotifyVia {
	pub email: Option<String>,
	pub telegram: Option<ChatId>,
}

#[derive(Clone)]
pub struct Alarm {
	pub expression: String,
	pub weekday: Option<HashSet<Weekday>>,
	pub period: Option<Duration>,
	pub message: Option<String>,
	pub command: Option<String>,
	pub tz_offset: i32, //in hours to the east
	pub notify: NotifyVia,
}

pub struct CompiledAlarm {
	expression: evalexpr::Node,
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
			weekday, period, 
			message, command, 
			tz_offset, notify} = alarm;
		let timezone = FixedOffset::east(tz_offset*3600); 
		let expression = build_operator_tree(&expression).unwrap();
		let period = period.map(|d| (d, Instant::now()));

		CompiledAlarm {
			expression,
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
	pub async fn evalute(&self, context: &mut evalexpr::HashMapContext, 
		now: &DateTime::<Utc>) -> Result<(), AlarmError> {
		
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
		match self.expression.eval_boolean_with_context(context){
			Ok(alarm) => if alarm {self.sound_alarm().await?; dbg!();},
			Err(error) => match error {
					VariableIdentifierNotFound(_) => {
						//if happens for long time warn user
					}
					_ => {error!("{:?}", error); dbg!();},
			}
		}
		Ok(())
	}

	async fn sound_alarm(&self) -> Result<(), AlarmError>{
		if let Some(_email) = &self.notify.email {
			todo!();
		}
		if let Some(chat_id) = &self.notify.telegram {
			if let Some(message) = &self.message {
				bot::send_text_reply(*chat_id, TOKEN, message).await?;
			} else {
				let text = format!("alarm: {}", self.expression);
				bot::send_text_reply(*chat_id, TOKEN, text).await?;
			}
			if let Some(command) = &self.command {
				todo!();
				//let user_id = ;
				//let state = ;
				//bot::handle_command(command, chat_id, user_id, state);
			}
		}
		Ok(())
	}
}


#[derive(Message)]
#[rtype(result = "Result<(),AlarmError>")]
pub struct AddAlarm {
    pub alarm: Alarm,
    pub username: String,
    pub sets: Vec<DatasetId>,
}

impl Handler<AddAlarm> for DataRouter {
	type Result = Result<(),AlarmError>;

	fn handle(&mut self, msg: AddAlarm, _: &mut Context<Self>) -> Self::Result {
		let mut set_id_alarm = Vec::with_capacity(msg.sets.len()); 
        for set_id in msg.sets {
			let list = if let Some(list) = self.alarms_by_set.get_mut(&set_id){
				list
			} else {
				self.alarms_by_set.insert(set_id, HashMap::new());
				self.alarms_by_set.get_mut(&set_id).unwrap()
			};
            
            let free_id = (std::u8::MIN..std::u8::MAX)
                .skip_while(|x| list.contains_key(x))
                .next().ok_or(AlarmError::TooManyAlarms)?;
			
			let alarm: CompiledAlarm = msg.alarm.clone().into();
			list.insert(free_id, (alarm, msg.username.clone()));
            set_id_alarm.push((set_id, free_id, msg.alarm.clone()));
        }
		self.alarms_by_username.insert(msg.username, set_id_alarm);
		//TODO sync changes to disk
		Ok(())
	}
}

#[derive(Message)]
#[rtype(result = "Option<Vec<(DatasetId, Id, Alarm)>>")]
pub struct ListAlarms {
    pub username: String,
}

impl Handler<ListAlarms> for DataRouter {
	type Result = Option<Vec<(DatasetId, Id, Alarm)>>;

	fn handle(&mut self, msg: ListAlarms, _: &mut Context<Self>) -> Self::Result {
		let list = self.alarms_by_username
			.get(&msg.username).map(|set| set.iter()
					.map(|(set_id, alarm_id, alarm)| 
						(*set_id, *alarm_id, alarm.clone())
				).collect());
        list
	}
}
/*
#[derive(Message)]
struct RemoveAlarm {
	
}

impl Handler<RemoveAlarm> for DataRouter {
	type Result = ();

	fn handle(&mut self, msg: RemoveAlarm, _: &mut Context<Self>) -> Self::Result {

	}
}*/