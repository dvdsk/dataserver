use crate::bot::{send_text_reply, Error};
use crate::databases::User;
use telegram_bot::types::refs::ChatId;

use crate::data_store::data_router::DataRouterState;
use bitspec::FieldId;

pub const USAGE: &str = "/plotables";
pub const DESCRIPTION: &str = "shows all possible data input for the plot function";
pub async fn send(
	chat_id: ChatId,
	user: &User,
	state: &DataRouterState,
	token: &str,
) -> Result<(), Error> {
	let mut text = String::default();
	const HEADER: &str = "\n<plotable id> <plotable name>\n";

	let data = state.data.read().unwrap();
	for (dataset_id, authorized_fields) in user.timeseries_with_access.iter() {
		let metadata = &data.sets.get(&dataset_id).unwrap().metadata;
		text.push_str(&metadata.name);
		text.push_str(HEADER);

		for field_id in authorized_fields.iter().map(|f| FieldId::from(f) as usize) {
			let field = &metadata.fields[field_id];
			text.push_str(&format!(
				"{}_{}\t\t\t{}\n",
				dataset_id, field_id, field.name
			));
		}
	}

	if text.is_empty() {
		text.push_str("you have no plotables")
	}
	send_text_reply(chat_id, token, text).await
}
