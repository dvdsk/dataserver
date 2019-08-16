
//candidade for sending: https://crates.io/crates/awesome-bot
//actix (old) based: https://github.com/jeizsm/actix-telegram/blob/master/examples/server.rs
// https://crates.io/crates/telegram-bot-raw


//plot generation using: https://github.com/38/plotters/tree/master/examples

use actix_web::web::{HttpResponse, Data, Form, Bytes};
use actix_web::http::{StatusCode, header};

use reqwest;
use log::{warn, info ,error};
use serde::Deserialize;
use serde_json;

use telegram_bot::types::update::Update;
use telegram_bot::types::requests::send_message::SendMessage;
use telegram_bot::types::requests::_base::{HttpRequest, Request};
use telegram_bot::types::ToChatRef;
use telegram_bot::types::update::UpdateKind;
use telegram_bot::types::HttpResponse as telegramResponse;

use crate::httpserver::InnerState;

pub fn handle_bot_message<T: InnerState+'static>(state: Data<T>, raw_update: Bytes)
	 -> HttpResponse {
	
	//TODO make actix web deserialise bot messages to: 
	//"telegram_bot::types::update::Update", then we can handle upon that object

    dbg!("got telegrambot message");
	//FIXME TODO
	pub const TOKEN: &str = "109451485:AAE6Yghjq1qJsxu75uureFkvaMB_Zrt7YsY";

	let update: Update = serde_json::from_slice(&raw_update.to_vec()).unwrap();

	match &update.kind{
	 	UpdateKind::Message(message) => {
			dbg!(&message.kind);
			let testmessage = format!("{{\"method\": sendMessage, \"chat_id\": {chat_id}, \"text\": {text}}}"
				,chat_id=message.chat.id()
				,text="hello world2");

			//dbg!(&testmessage);
			test_delivery(message, TOKEN);
			HttpResponse::Ok()
				.status(StatusCode::OK)
				//.set_header(header::CONTENT_TYPE, "application/json")
				//.body(testmessage)
				.finish()
				//TODO try using hand formatted test message
		}
	 	_ => {
			warn!("unhandled message type");
			HttpResponse::Ok()
				.status(StatusCode::OK)
				.body("{}")
		 }
	}
}

fn test_delivery(message: &telegram_bot::types::message::Message, token: &str){
	let url = format!("https://api.telegram.org/bot{}/sendMessage", token);
	let testmessage = format!(r#"{{"chat_id": {chat_id}, "text": {text}}}"#
		,chat_id=message.chat.id()
		,text="hello world");

	let testmessage = r#"{"chat_id": 15997283, "text": "hello world"}"#;
	let test = message.chat.id().to_string();
	let params = [("chat_id", test.as_str())
	             ,("text", "hello world")];

	println!("{}", &testmessage);
	let client = reqwest::Client::new();
	let mut res = client.post(&url)
		.body(testmessage)
	    //.form(&params)
		.header(reqwest::header::CONTENT_TYPE, "application/json")
		.send().unwrap();
	dbg!(&res);
	dbg!(res.text());

	//let client_debug = client.post(&url).form(&params);
	//dbg!(client_debug);
}

#[derive(Debug)]
pub enum BotError{
	HttpClientError(reqwest::Error),
	CouldNotSetWebhook,
	InvalidServerResponse,
}

impl From<reqwest::Error> for BotError {
	fn from(error: reqwest::Error) -> Self {
		BotError::HttpClientError(error)
	}
}

fn send_test_reply<C: ToChatRef>(chat: C, token: &str) -> Result<(), BotError>{//add as arg generic ToChatRef (should get from Update)
	//TODO create a SendMessage, serialise it (use member function serialize) 
	//then use the HttpRequest fields, (url, method, and body) to send to telegram
	let url = format!("https://api.telegram.org/bot{}/sendMessage", token);	
	let text = String::from("hi");
	let body = serde_json::to_string(&SendMessage::new(chat, text)).unwrap();
	dbg!(&body);

	let client = reqwest::Client::new();
	let res = client.post(&url)
			.body(body)
			.send()?;
	
	if res.status() != reqwest::StatusCode::OK {
		dbg!(res);
		Err(BotError::InvalidServerResponse)
	} else {
		info!("send message");
		Ok(())
	}
}

pub fn set_webhook(domain: &str, token: &str) -> Result<(), BotError> {
	let url = format!("https://api.telegram.org/bot{}/setWebhook", token);
	let webhook_url = format!("{}:8443/{}",domain, token);

	let params = [("url", &webhook_url)];
	let client = reqwest::Client::new();
	let res = client.post(url.as_str())
	      .form(&params)
		  .send()?;
	
	if res.status() != reqwest::StatusCode::OK {
		dbg!(res);
		Err(BotError::CouldNotSetWebhook)
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