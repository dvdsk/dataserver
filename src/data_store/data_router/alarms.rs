
use actix::prelude::*;
use log::error;
use evalexpr::{self, 
	HashMapContext, 
	Context as evalContext,
	build_operator_tree,
	error::EvalexprError::VariableIdentifierNotFound};
use telegram_bot::types::refs::ChatId;

use super::DataRouter;
use crate::data_store::DatasetId;
use crate::bot::alarms;

enum AlarmError {
    TooManyAlarms,
}

pub type Id = u8;
#[derive(Clone)]
struct NotifyVia {
	email: Option<String>,
	telegram: Option<ChatId>,
}

#[derive(Clone)]
pub struct Alarm {
	expression: String,
	notify: NotifyVia,
	message: String,
}

pub struct CompiledAlarm {
	expression: evalexpr::Node,
	notify: NotifyVia,
	message: String,	
}

impl From<Alarm> for CompiledAlarm {
	fn from(alarm: Alarm) -> Self {
		let Alarm {expression, notify, message} = alarm;
		let expression = build_operator_tree(&expression).unwrap();
		
		CompiledAlarm {
			expression, 
			notify, 
			message
		}
	}
}

impl CompiledAlarm {
	pub fn evalute(&self, context: &evalexpr::HashMapContext) {
		match self.expression.eval_boolean_with_context(context){
			Ok(alarm) => if alarm {self.sound_alarm();},
			Err(error) => match error {
					VariableIdentifierNotFound(_) => return,
					_ => error!("{:?}", error),
			}
		}
	}

	fn sound_alarm(&self) {
		if let Some(_email) = &self.notify.email {
			todo!();
		}
		if let Some(chat_id) = &self.notify.telegram {

			todo!();
		}
	}
}


struct AddAlarm {
    alarm: Alarm,
    username: String,
    sets: Vec<DatasetId>,
}

impl Message for AddAlarm {
    type Result = Result<(),AlarmError>;
}

impl Handler<AddAlarm> for DataRouter {
	type Result = Result<(),AlarmError>;

	fn handle(&mut self, msg: AddAlarm, _: &mut Context<Self>) -> Self::Result {
		let mut set_id_alarm = Vec::with_capacity(msg.sets.len()); 
        for set_id in msg.sets {
            let list = self.alarms_by_set.get_mut(&set_id).unwrap();
            
            let free_id = (std::u8::MIN..std::u8::MAX)
                .skip_while(|x| list.contains_key(x))
                .next().ok_or(AlarmError::TooManyAlarms)?;
			
			let alarm: CompiledAlarm = msg.alarm.clone().into();
			list.insert(free_id, (alarm, msg.username.clone())).unwrap();
            set_id_alarm.push((set_id, free_id, msg.alarm.clone()));
        }
		self.alarms_by_username.insert(msg.username, set_id_alarm).unwrap();
		
		Ok(())
	}
}

struct ListAlarms {
    username: String,
}

impl Message for ListAlarms {
    type Result = Result<Vec<Alarm>,()>;
}

impl Handler<ListAlarms> for DataRouter {
	type Result = Result<Vec<Alarm>,()>;

	fn handle(&mut self, msg: ListAlarms, _: &mut Context<Self>) -> Self::Result {
		let list: Vec<Alarm> = self.alarms_by_username
			.get(&msg.username).unwrap().iter()
			.map(|(_, _, alarm)| alarm.clone())
			.collect();

        Ok(list)
	}
}

#[derive(Message)]
struct RemoveAlarm {
	
}

impl Handler<RemoveAlarm> for DataRouter {
	type Result = ();

	fn handle(&mut self, msg: RemoveAlarm, _: &mut Context<Self>) -> Self::Result {

	}
}