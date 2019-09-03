use crate::databases::BotUserInfo;
use crate::bot::Error;
use telegram_bot::types::refs::ChatId;

use super::super::send_text_reply;

const USAGE: &str = "/help: shows this list";
pub fn send(chat_id: ChatId, user_info: &BotUserInfo, token: &str)
	-> Result<(), Error> {
	let aliasses = &user_info.aliases;

	const HELP_TEXT: &str = "List of commands:
/test: replies the text \"hi\"
/plot <plotable> <number><s|m|h|d|w|monthes|years>: creates a line graph of a sensor value from a given time ago till now
/help: shows this list
/plotables: shows all possible input for the plot function
/show <plotable>: sends back the current value of the requested sensor value";
//TODO add alarms (arm disarm etc)
//TODO man pages?

	let mut text = String::from(HELP_TEXT);
	for (alias, alias_expanded) in aliasses.iter() {
		text.push_str(&format!("\nconfigured aliasses:\n {}: {}\n",alias,alias_expanded));
	}
	send_text_reply(chat_id, token, text)?;
	Ok(())
}