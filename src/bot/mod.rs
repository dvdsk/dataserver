use actix_web::web::{HttpResponse, Data, Bytes};
use actix_web::http::{StatusCode};

use reqwest;
use log::{warn, info ,error};
use serde::Deserialize;
use serde_json;

use telegram_bot::types::update::Update;
use telegram_bot::types::update::UpdateKind;
use telegram_bot::types::message::MessageKind;
use telegram_bot::types::refs::{ChatId, UserId};

use crate::httpserver::{InnerState, DataRouterState};
use crate::databases::{BotUserDatabase, BotUserInfo, UserDbError};

mod commands;
use commands::{plot, help, plotables, show};

pub const TOKEN: &str = "109451485:AAE6Yghjq1qJsxu75uureFkvaMB_Zrt7YsY";

#[derive(Debug)]
pub enum Error{
	HttpClientError(reqwest::Error),
	CouldNotSetWebhook,
	InvalidServerResponse(reqwest::Response),
	UnhandledUpdateKind,
	UnhandledMessageKind,
	BotDatabaseError(UserDbError),
	UnknownAlias(String),
	PlotError(plot::Error),
	ShowError(show::Error),
}

impl From<show::Error> for Error {
	fn from(error: show::Error) -> Self {
		Error::ShowError(error)
	}
}

impl From<plot::Error> for Error {
	fn from(error: plot::Error) -> Self {
		Error::PlotError(error)
	}
}

impl From<reqwest::Error> for Error {
	fn from(error: reqwest::Error) -> Self {
		Error::HttpClientError(error)
	}
}

impl From<UserDbError> for Error {
	fn from(error: UserDbError) -> Self {
		Error::BotDatabaseError(error)
	}
}

fn to_string_and_ids(update: Update) -> Result<(String, ChatId, UserId),Error>{
	if let UpdateKind::Message(message) = update.kind {
		let chat_id = message.chat.id();
		let user_id = message.from.id;
		if let MessageKind::Text{data, entities} = message.kind {
			return Ok((data, chat_id, user_id));
		} else {
			warn!("unhandled message kind");
			return Err(Error::UnhandledUpdateKind);
		}
	} else {
		warn!("unhandled update from telegram: {:?}", update);
		return Err(Error::UnhandledMessageKind);
	}
}

fn resolve_alias(possible_alias: &str, userinfo: &BotUserInfo, user_id: UserId) -> Result<Option<String>, Error> {
	if let Some(alias) = userinfo.aliases.get(possible_alias){
		Ok(Some(alias.to_string()))
	} else {
		Ok(None)
	}
}

fn handle_command<T: InnerState>(mut text: String, chat_id: ChatId, user_id: UserId, state: &DataRouterState) -> Result<(), Error>{
	let userinfo = state.bot_user_db.get_userdata(user_id)?;

	loop {
		let mut args = text.as_str().split_whitespace();
		let command = args.next().unwrap_or_default();
		match command {
			"/test" => {
				send_text_reply(chat_id, TOKEN, "hi")?; 
				break;
			}
			"/plot" => {
				plot::send(chat_id, user_id, state, TOKEN, args, &userinfo)?; 
				break;
			}
			"/help" => {
				help::send(chat_id, &userinfo, TOKEN)?; 
				break;
			}
			"/plotables" => {
				plotables::send(chat_id, &userinfo, state, TOKEN)?;
				break;
			}
			"/show" => {
				show::send(chat_id, user_id, state, TOKEN, args, &userinfo)?;
				break;
			}
			&_ => {}
		}
		if let Some(alias_text) = resolve_alias(command, &userinfo, user_id)?{
			text = alias_text; //FIXME //TODO allows loops in aliasses, thats fun right? (fix after fun)
		} else {
			warn!("no known command or alias: {:?}", command);
			return Err(Error::UnknownAlias(command.to_owned()));
		}
	}
	Ok(())
}

fn handle_error(error: Error, chat_id: ChatId, user_id: UserId) {
	let error_message = match error {
		Error::BotDatabaseError(error) => error.to_text(user_id),
		Error::PlotError(error) => error.to_text(user_id),
		Error::ShowError(error) => error.to_text(user_id),
		Error::UnknownAlias(alias_text) => 
			format!("your input: \"{}\", is not a possible command or a configured alias. Use /help to get a list of possible commands and configured aliasses", alias_text),		
		_ => {
			error!("Internal error during handling of commands: {:?}", error);
			format!("apologies, an internal error happend this has been reported and will be fixed as soon as possible")
		}	
	};
	if let Err(error) = send_text_reply(chat_id, TOKEN, error_message){
		error!("Could not send text reply to user: {:?}", error);
	}
}

pub fn handle_bot_message<T: InnerState+'static>(state: Data<T>, raw_update: Bytes)
	 -> HttpResponse {
	
	//TODO make actix web deserialise bot messages to: 
	//"telegram_bot::types::update::Update", then we can handle upon that object

    dbg!("got telegrambot message");
	//FIXME TODO

	let update: Update = serde_json::from_slice(&raw_update.to_vec()).unwrap();
	if let Ok((text, chat_id, user_id)) = to_string_and_ids(update){
		if let Err(error) = handle_command::<T>(text, chat_id, user_id, state.inner_state()){
			handle_error(error, chat_id, user_id);
		}
	}

	HttpResponse::Ok()
		.status(StatusCode::OK)
		.body("{}")
}

fn send_text_reply<T: Into<String>>(chat_id: ChatId, token: &str, text: T) -> Result<(), Error>{//add as arg generic ToChatRef (should get from Update)
	//TODO create a SendMessage, serialise it (use member function serialize) 
	//then use the HttpRequest fields, (url, method, and body) to send to telegram
	let url = format!("https://api.telegram.org/bot{}/sendMessage", token);	
	let form = reqwest::multipart::Form::new()
		.text("chat_id", chat_id.to_string())
		.text("text", text.into());

	let client = reqwest::Client::new();
	let resp = client.post(&url)
		.multipart(form).send()?;
	//https://stackoverflow.com/questions/57540455/error-blockingclientinfuturecontext-when-trying-to-make-a-request-from-within
	
	if resp.status() != reqwest::StatusCode::OK {
		Err(Error::InvalidServerResponse(resp))
	} else {
		info!("send message");
		Ok(())
	}
}

pub fn set_webhook(domain: &str, token: &str) -> Result<(), Error> {
	let url = format!("https://api.telegram.org/bot{}/setWebhook", token);
	let webhook_url = format!("{}:8443/{}",domain, token);

	let params = [("url", &webhook_url)];
	let client = reqwest::Client::new();
	let res = client.post(url.as_str())
	      .form(&params)
		  .send()?;
	
	if res.status() != reqwest::StatusCode::OK {
		dbg!(res);
		Err(Error::CouldNotSetWebhook)
	} else {
		info!("set webhook to: {}", webhook_url);
		Ok(())
	}
}