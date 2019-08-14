
//candidade for sending: https://crates.io/crates/awesome-bot
//actix (old) based: https://github.com/jeizsm/actix-telegram/blob/master/examples/server.rs
// https://crates.io/crates/telegram-bot-raw


//plot generation using: https://github.com/38/plotters/tree/master/examples

use actix_web::web::{HttpResponse, Data, Bytes, Form};
use actix_web::http::StatusCode;

use reqwest;

use telegram_bot::types::update::Update;
use telegram_bot::types::requests::send_message::SendMessage;
use telegram_bot::types::requests::_base::HttpRequest;

use crate::httpserver::InnerState;

const TOKEN: &str = "some token";

pub fn handle_bot_message<T: InnerState+'static>(state: Data<T>, update: Form<Update>)
	 -> HttpResponse {
	
	//TODO make actix web deserialise bot messages to: 
	//"telegram_bot::types::update::Update", then we can handle upon that object

    dbg!("got telegrambot message");
	dbg!(update);
	send_test_reply();

	HttpResponse::Ok().status(StatusCode::OK).finish()
}

fn send_test_reply<C: ToChatRef>(chat: C) {//add as arg generic ToChatRef (should get from Update)
	//TODO create a SendMessage, serialise it (use member function serialize) 
	//then use the HttpRequest fields, (url, method, and body) to send to telegram
	let text = String::from("hi");
	let request = SendMessage::new(chat, text).serialize().unwrap();
	let request_url, body, method = request;

	match body {
		Empty => dbg!("ERROR");
		Json(body) => {
			let client = reqwest::Client::new();
			client.post(request_url.url(TOKEN))
			      .body(body);
			dbg!("send message")
		}
	}

}

pub fn set_webhook(){

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