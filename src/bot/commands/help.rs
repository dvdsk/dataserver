use crate::databases::BotUserInfo;
use crate::bot::Error;
use telegram_bot::types::refs::ChatId;

use super::super::send_text_reply;
use super::{plotables, show, alias, keyboard};

#[cfg(feature = "plotting")]
use super::plot;

const USAGE: &str = "/help";
const DESCRIPTION: &str = "shows this list";
pub fn send(chat_id: ChatId, user_info: &BotUserInfo, token: &str)
	-> Result<(), Error> {
	let aliasses = &user_info.aliases;

	#[cfg(feature = "plotting")]
	let mut text = format!("{}\n\t{}\n\t{}\n\t{}\n\t{}\n\t{}\n\t{}\n\t{}\n{}\n\t{}\n{}\n\t{}\n{}\n\t{}\n{}\n\t{}\n",
		USAGE, DESCRIPTION,
		plot::USAGE, plot::DESCRIPTION,
		plotables::USAGE, plotables::DESCRIPTION,
		show::USAGE, show::DESCRIPTION,
		alias::USAGE, alias::DESCRIPTION,
		keyboard::USAGE_SHOW, keyboard::DESCRIPTION_SHOW,
		keyboard::USAGE_ADD, keyboard::DESCRIPTION_ADD,
		keyboard::USAGE_REMOVE, keyboard::DESCRIPTION_REMOVE,
		);

	#[cfg(not(feature = "plotting"))]
	let mut text = format!("{}\n\t{}\n\t{}\n\t{}\n\t{}\n\t{}\n\t{}\n\t{}\n{}\n\t{}\n{}\n\t{}\n{}\n\t{}\n",
		USAGE, DESCRIPTION,
		plotables::USAGE, plotables::DESCRIPTION,
		show::USAGE, show::DESCRIPTION,
		alias::USAGE, alias::DESCRIPTION,
		keyboard::USAGE_SHOW, keyboard::DESCRIPTION_SHOW,
		keyboard::USAGE_ADD, keyboard::DESCRIPTION_ADD,
		keyboard::USAGE_REMOVE, keyboard::DESCRIPTION_REMOVE,
		);

	text.push_str("\nconfigured aliasses:\n");
	for (alias, alias_expanded) in aliasses.iter() {
		text.push_str(&format!(" {}: {}\n",alias,alias_expanded));
	}
	send_text_reply(chat_id, token, text)?;
	Ok(())
}