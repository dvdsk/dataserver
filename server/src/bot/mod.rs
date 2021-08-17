use actix_web::http::StatusCode;
use actix_web::web::{Bytes, Data, HttpResponse};

use log::{error, info, warn};

use telegram_bot::types::message::MessageKind;
use telegram_bot::types::refs::{ChatId, UserId};
use telegram_bot::types::update::Update;
use telegram_bot::types::update::UpdateKind;

use crate::data_store::data_router::DataRouterState;
use crate::database::{User, UserDbError};

pub mod commands;
pub use commands::alarms;

use commands::plot;
use commands::{alias, help, keyboard, plotables, show};
use error_level::ErrorLevel;

async fn handle_error(error: Error, chat_id: ChatId, token: &str) {
    error.log_error();
	let error_message = error.to_string();
	if let Err(error) = send_text_reply(chat_id, token, error_message).await {
		error!("Could not send text reply to user: {:?}", error);
	}
}

const INT_ERR_TEXT: &str = "apologies, an internal error happend this has been reported and will be fixed as soon as possible";
#[derive(ErrorLevel, thiserror::Error, Debug)]
pub enum Error {
	#[report(error)]
    #[error("{}", INT_ERR_TEXT)]
	HttpClientError(#[from] reqwest::Error),
	#[report(error)]
	#[error("could not set up webhook for telegram bot")]
	CouldNotSetWebhook,
	#[report(warn)]
	#[error("{}", INT_ERR_TEXT)]
	InvalidServerResponse(reqwest::Response),
	#[report(warn)]
	#[error("{}", INT_ERR_TEXT)]
	InvalidServerResponseBlocking(reqwest::blocking::Response),
	#[report(no)]
	#[error("sorry I can not understand your input")]
	UnhandledUpdateKind,
	#[report(no)]
	#[error("sorry I can not understand your input")]
	UnhandledMessageKind,
	#[error("sorry I can not understand your input")]
	BotDatabaseError(#[from] UserDbError),
	#[report(no)]
	#[error(
		"your input: \"{0}\", is not a possible command or \
                a configured alias. Use /help to get a list of possible \
                commands and configured aliases"
	)]
	UnknownAlias(String),
	#[error("{0}")]
	ShowError(#[from] show::Error),
	#[error("{0}")]
	AliasError(#[from] alias::Error),
	#[error("{0}")]
	KeyBoardError(#[from] keyboard::Error),
	#[error("{0}")]
	AlarmError(#[from] alarms::Error),
	#[error("{0}")]
	PlotError(#[from] plot::Error),
}

fn to_string_and_ids(update: Update) -> Result<(String, ChatId, UserId), Error> {
	if let UpdateKind::Message(message) = update.kind {
		let chat_id = message.chat.id();
		let user_id = message.from.id;
		if let MessageKind::Text { data, entities: _ } = message.kind {
			Ok((data, chat_id, user_id))
		} else {
			warn!("unhandled message kind");
			Err(Error::UnhandledUpdateKind)
		}
	} else {
		warn!("unhandled update from telegram: {:?}", update);
		Err(Error::UnhandledMessageKind)
	}
}

fn resolve_alias(possible_alias: &str, user: &User) -> Result<Option<String>, Error> {
	if let Some(alias) = user.aliases.get(possible_alias) {
		Ok(Some(alias.to_string()))
	} else {
		Ok(None)
	}
}

async fn handle_command(
	mut text: String,
	chat_id: ChatId,
	user_id: UserId,
	state: &DataRouterState,
) -> Result<(), Error> {
	let token = &state.bot_token;
	let db_id = state.db_lookup.by_telegram_id(&user_id)?;
	let user = state.user_db.get_user(db_id)?;

	loop {
		let split = text.find(char::is_whitespace);
		let mut command = text;
		let args = command.split_off(split.unwrap_or_else(|| command.len()));
		match command.as_str() {
			"/test" => {
				send_text_reply(chat_id, token, "hi").await?;
				break;
			}
			//TODO needs to use threadpool
			"/plot" => {
				plot::send(chat_id, state, token, args, &user).await?;
				break;
			}
			"/help" => {
				help::send(chat_id, &user, token).await?;
				break;
			}
			"/plotables" => {
				plotables::send(chat_id, &user, state, token).await?;
				break;
			}
			"/show" => {
				show::send(chat_id, state, token, args, &user).await?;
				break;
			}
			"/keyboard" => {
				keyboard::show(chat_id, token, user).await?;
				break;
			}
			"/keyboard_add" => {
				keyboard::add_button(chat_id, state, token, args, user).await?;
				break;
			}
			"/keyboard_remove" => {
				keyboard::remove_button(chat_id, state, token, args, user).await?;
				break;
			}
			"/alarm" => {
				alarms::handle(chat_id, token, args, user, state).await?;
				break;
			}
			"/alias" => {
				alias::send(chat_id, state, token, args, user).await?;
				break;
			}
			&_ => {}
		}
		if let Some(alias_text) = resolve_alias(&command, &user)? {
			text = alias_text; //FIXME //TODO allows loops in aliasses, thats fun right? (fix after fun)
		} else {
			warn!("no known command or alias: {:?}", &command);
			return Err(Error::UnknownAlias(command));
		}
	}
	Ok(())
}

async fn handle(update: Update, state: &DataRouterState) {
	let token = &state.bot_token;
	if let Ok((text, chat_id, user_id)) = to_string_and_ids(update) {
		if let Err(error) = handle_command(text, chat_id, user_id, &state).await {
			handle_error(error, chat_id, token).await;
		}
	}
}

pub async fn handle_webhook(state: Data<DataRouterState>, raw_update: Bytes) -> HttpResponse {
	let update: Update = serde_json::from_slice(&raw_update.to_vec()).unwrap();
	handle(update, state.get_ref()).await;

	HttpResponse::Ok().status(StatusCode::OK).body("{}")
}

pub async fn send_text_reply<T: Into<String>>(
	chat_id: ChatId,
	token: &str,
	text: T,
) -> Result<(), Error> {
	//add as arg generic ToChatRef (should get from Update)
	//TODO create a SendMessage, serialise it (use member function serialize)
	//then use the HttpRequest fields, (url, method, and body) to send to telegram
	let url = format!("https://api.telegram.org/bot{}/sendMessage", token);
	let form = reqwest::multipart::Form::new()
		.text("chat_id", chat_id.to_string())
		.text("text", text.into());

	let client = reqwest::Client::new();
	let resp = client.post(&url).multipart(form).send().await?;
	//https://stackoverflow.com/questions/57540455/error-blockingclientinfuturecontext-when-trying-to-make-a-request-from-within

	if resp.status() != reqwest::StatusCode::OK {
		Err(Error::InvalidServerResponse(resp))
	} else {
		info!("send message");
		Ok(())
	}
}

pub fn send_text_reply_blocking<T: Into<String>>(
	chat_id: ChatId,
	token: &str,
	text: T,
) -> Result<(), Error> {
	//add as arg generic ToChatRef (should get from Update)
	//TODO create a SendMessage, serialise it (use member function serialize)
	//then use the HttpRequest fields, (url, method, and body) to send to telegram
	let url = format!("https://api.telegram.org/bot{}/sendMessage", token);
	let form = reqwest::blocking::multipart::Form::new()
		.text("chat_id", chat_id.to_string())
		.text("text", text.into());

	let client = reqwest::blocking::Client::new();
	let resp = client.post(&url).multipart(form).send()?;
	//https://stackoverflow.com/questions/57540455/error-blockingclientinfuturecontext-when-trying-to-make-a-request-from-within

	if resp.status() != reqwest::StatusCode::OK {
		Err(Error::InvalidServerResponseBlocking(resp))
	} else {
		info!("send message");
		Ok(())
	}
}

pub async fn set_webhook(domain: &str, token: &str, port: u16) -> Result<(), Error> {
	let url = format!("https://api.telegram.org/bot{}/setWebhook", token);
	let webhook_url = format!("{}:{}/{}", domain, port, token);
    dbg!(&url,&webhook_url);

	let params = [("url", &webhook_url)];
	let client = reqwest::Client::new();
	let res = client.post(url.as_str()).form(&params).send().await?;

	if res.status() != reqwest::StatusCode::OK {
        dbg!(&res);
		Err(Error::CouldNotSetWebhook)
	} else {
		info!("set webhook to: {}", webhook_url);
		Ok(())
	}
}
