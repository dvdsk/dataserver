pub const USAGE: &str = "/alias <name> <exiting command>";
pub const DESCRIPTION: &str = "this defines an new command that can be used to call an existing command with or without arguments. Leave the <existing command> paramater empty to remove an existing alias";

use crate::data_store::data_router::DataRouterState;
use crate::databases::User;
use log::error;
use telegram_bot::types::refs::ChatId;

use super::super::send_text_reply;
use super::super::Error as botError;

#[derive(thiserror::Error, Debug)]
pub enum Error {
	#[error("not enough arguments")]
	NotEnoughArguments,
	#[error("could not update database during setting of alias")]
	DbError(crate::databases::UserDbError),
}

pub async fn send(
	chat_id: ChatId,
	state: &DataRouterState,
	token: &str,
	args: String,
	mut user: User,
) -> Result<(), botError> {
	let mut args = args.split_whitespace();
	let alias_name = args.next().ok_or(Error::NotEnoughArguments)?.to_owned();

	let mut command = String::default();
	args.for_each(|arg| {
		command.push_str(arg);
		command.push(' ')
	});

	let mut text = String::default();
	if command.len() == 0 {
		if let Some(old_command) = user.aliases.remove(&alias_name) {
			state
				.user_db
				.set_user(user.clone())
				.await
				.map_err(|e| Error::DbError(e))?;
			text.push_str(&format!("unset \"{}\" {}", alias_name, old_command));
		} else {
			text.push_str("did not unset alias as non was set");
		}
	} else {
		if let Some(old_command) = user.aliases.insert(alias_name, command) {
			text.push_str(&format!(
				"new alias set, overwrote previous value: {}",
				old_command
			));
		} else {
			text.push_str("new alias set");
		}
		state
			.user_db
			.set_user(user.clone())
			.await
			.map_err(|e| Error::DbError(e))?;
	}

	send_text_reply(chat_id, token, text).await?;
	Ok(())
}
