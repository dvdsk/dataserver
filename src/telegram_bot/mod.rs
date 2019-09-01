
//candidade for sending: https://crates.io/crates/awesome-bot
//actix (old) based: https://github.com/jeizsm/actix-telegram/blob/master/examples/server.rs
// https://crates.io/crates/telegram-bot-raw


//plot generation using: https://github.com/38/plotters/tree/master/examples

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

use crate::httpserver::InnerState;
mod botplot;

#[derive(Debug)]
pub enum Error{
	HttpClientError(reqwest::Error),
	CouldNotSetWebhook,
	InvalidServerResponse,
	UnhandledUpdateKind,
	UnhandledMessageKind,
}

impl From<reqwest::Error> for Error {
	fn from(error: reqwest::Error) -> Self {
		Error::HttpClientError(error)
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

pub fn handle_bot_message<T: InnerState+'static>(state: Data<T>, raw_update: Bytes)
	 -> HttpResponse {
	
	//TODO make actix web deserialise bot messages to: 
	//"telegram_bot::types::update::Update", then we can handle upon that object

    dbg!("got telegrambot message");
	//FIXME TODO
	pub const TOKEN: &str = "109451485:AAE6Yghjq1qJsxu75uureFkvaMB_Zrt7YsY";

	let update: Update = serde_json::from_slice(&raw_update.to_vec()).unwrap();

	let (text, chat_id, user_id) = to_string_and_ids(update).unwrap();
	let command = text.as_str().split_whitespace().next().unwrap_or_default();
	match command {
		"test" => send_test_reply(chat_id, TOKEN).unwrap(),
		"plot" => send_plot(chat_id, user_id, state.inner_state(), TOKEN, text).unwrap(),
		&_ => warn!("no known command in: {:?}", text),
	}

	HttpResponse::Ok()
		.status(StatusCode::OK)
		.body("{}")
}

fn send_plot(chat_id: ChatId, user_id: UserId, state: &InnerState, token: &str, text: String)
	 -> Result<(), Error>{

	let plot = botplot::plot(text, state.inner_state()).unwrap();

	let photo_part = reqwest::multipart::Part::bytes(plot)
		.mime_str("image/png").unwrap()
		.file_name("testplot.png");

	let url = format!("https://api.telegram.org/bot{}/sendPhoto", token);

	let form = reqwest::multipart::Form::new()
		.text("chat_id", chat_id.to_string())
		.part("photo", photo_part);

	let client = reqwest::Client::new();
	let resp = client.post(&url)
		.multipart(form).send()?;

	Ok(())
}

fn send_test_reply(chat_id: ChatId, token: &str) -> Result<(), Error>{//add as arg generic ToChatRef (should get from Update)
	//TODO create a SendMessage, serialise it (use member function serialize) 
	//then use the HttpRequest fields, (url, method, and body) to send to telegram
	let url = format!("https://api.telegram.org/bot{}/sendMessage", token);	
	let text = String::from("hi");
	let form = reqwest::multipart::Form::new()
		.text("chat_id", chat_id.to_string())
		.text("text", text);

	let client = reqwest::Client::new();
	let resp = client.post(&url)
		.multipart(form).send()?;
	
	if resp.status() != reqwest::StatusCode::OK {
		dbg!(resp);
		Err(Error::InvalidServerResponse)
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

/*
fn send_plot(){
	//"sendChatAction" photo (shows taking photo)
	//The status is set for 5 seconds or less (when a message arrives from your bot, Telegram clients clear its typing status).
	//keep sending every 5 seconds

	//send inputMediaPhoto with media string "attach://<file_attach_name>"
	//Post the file using multipart/form-data to "<file_attach_name>"
	//When sending by URL the target file must have the correct MIME type (e.g., audio/mpeg for sendAudio, etc.).
}*/