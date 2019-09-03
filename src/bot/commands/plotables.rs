use crate::databases::BotUserInfo;
use crate::bot::{Error, send_text_reply};
use telegram_bot::types::refs::ChatId;

use crate::httpserver::{DataRouterState, InnerState};
use crate::httpserver::timeseries_interface;
use timeseries_interface::FieldId;

pub fn send(chat_id: ChatId, userinfo: &BotUserInfo, state: &DataRouterState, token: &str)
     -> Result<(), Error> {
	let mut accessible_fields = String::from("");

	let data = state.inner_state().data.read().unwrap();	
    for (dataset_id, authorized_fields) in userinfo.timeseries_with_access.iter() {
        let metadata = &data.sets.get(&dataset_id).unwrap().metadata;

		for field_id in authorized_fields.iter().map(|f| FieldId::from(f) as usize) {
		    let field = &metadata.fields[field_id];
            accessible_fields.push_str(&format!("{}:{} {}", dataset_id, field_id, field.name));
		}
	}

	if accessible_fields.len() == 0 {
		accessible_fields.push_str("you have no plotables")
	}
	send_text_reply(chat_id, token, accessible_fields)
}