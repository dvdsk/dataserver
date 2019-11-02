use crate::databases::BotUserInfo;
use crate::bot::Error;
use telegram_bot::types::refs::ChatId;

use super::super::send_text_reply;
use super::{plotables, show, alias};

#[cfg(feature = "plotting")]
use super::plot;

const USAGE: &str = "/help";
const DESCRIPTION: &str = "shows this list";
pub fn send(chat_id: ChatId, user_info: &BotUserInfo, token: &str)
	-> Result<(), Error> {
	let aliasses = &user_info.aliases;

	#[cfg(feature = "plotting")]
	let mut text = format!("{}\n\t{}\n{}\n\t{}\n{}\n\t{}\n{}\n\t{}\n{}\n\t{}",
		USAGE, DESCRIPTION,
		plot::USAGE, plot::DESCRIPTION,
		plotables::USAGE, plotables::DESCRIPTION,
		show::USAGE, show::DESCRIPTION,
		alias::USAGE, alias::DESCRIPTION,
		);

	#[cfg(not(feature = "plotting"))]
	let mut text = format!("{}\n\t{}\n{}\n\t{}\n{}\n\t{}\n{}\n\t{}",
		USAGE, DESCRIPTION,
		plotables::USAGE, plotables::DESCRIPTION,
		show::USAGE, show::DESCRIPTION,
		alias::USAGE, alias::DESCRIPTION,
		);

	for (alias, alias_expanded) in aliasses.iter() {
		text.push_str(&format!("\nconfigured aliasses:\n {}: {}\n",alias,alias_expanded));
	}
	send_text_reply(chat_id, token, text)?;
	Ok(())
}