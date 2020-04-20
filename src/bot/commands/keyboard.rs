pub const USAGE_SHOW: &str = "/keyboard";
pub const USAGE_ADD: &str = "/keyboard_add <alias> ... <alias>";
pub const USAGE_REMOVE: &str = "/keyboard_remove <alias> ... <alias>";

pub const DESCRIPTION_SHOW: &str = "show the telegram keyboard";
pub const DESCRIPTION_ADD: &str = "add aliasses to the keyboard";
pub const DESCRIPTION_REMOVE: &str = "remove aliasses from the keyboard";

use std::collections::HashSet;
use itertools::Itertools;

use log::error;
use telegram_bot::types::refs::{ChatId, UserId};
use crate::databases::{User};
use crate::data_store::data_router::DataRouterState;

use crate::bot::Error as botError;

const MAX_ROW: usize = 3;
const MAX_COLUMN: usize = 4;
const CAPACITY: usize = MAX_ROW*MAX_COLUMN;

#[derive(Debug)]
pub enum Error{
	//NotEnoughArguments,
	NoKeyboardSet,
	NotEnoughSpace(usize),
	DbError(crate::databases::UserDbError),
}

impl Error {
	pub fn to_text(self, user_id: UserId) -> String {
		match self {
			/*Error::NotEnoughArguments => 
				format!("Not enough arguments, usage: {}", USAGE_SHOW),*/
			Error::NoKeyboardSet =>
				format!("No keyboard set, set one by adding a button: {}", USAGE_ADD),
			Error::NotEnoughSpace(free) =>
				format!("Not enough space on the keyboard, {} of {} spots left", 
				free, CAPACITY),
			Error::DbError(db_error) => {
				error!("could not update database for user_id: {} during setting of alias, error: {:?}", user_id, db_error);
				String::from("Internal error during setting of database")
			}
		}
	}
}

pub async fn show(chat_id: ChatId, token: &str, user: User)
    -> Result<(), botError>{
    
	reload(chat_id, token, user, "showing the user keyboard").await
}

async fn reload(chat_id: ChatId, token: &str, user: User, text: &str)
    -> Result<(), botError>{
    
	let keyboard_json = user.keyboard.ok_or(Error::NoKeyboardSet)?;
	let reply_markup = format!("{{\"keyboard\":{},\"resize_keyboard\": true}}", 
		keyboard_json);

    dbg!(&reply_markup);
	let url = format!("https://api.telegram.org/bot{}/sendMessage", token);	
	let form = reqwest::multipart::Form::new()
		.text("chat_id", chat_id.to_string())
		.text("text", String::from(text))
        .text("reply_markup", reply_markup);

	let client = reqwest::Client::new();
	let resp = client.post(&url)
		.multipart(form).send().await?;
    
    dbg!(&resp);
	if resp.status() != reqwest::StatusCode::OK {
		Err(botError::InvalidServerResponse(resp))
	} else {
		Ok(())
	}
}

//replykeyboardmarkup
type Keyboard = Vec<Vec<String>>;
pub async fn add_button(chat_id: ChatId, state: &DataRouterState, token: &str, 
    args: String, mut user: User)
     -> Result<(), botError> {

	let mut keyboard: Keyboard = //load or create keyboard
	if let Some(keyboard_str) = user.keyboard {
		serde_json::from_str(&keyboard_str).unwrap()
	} else {
		let mut new_kb = Vec::new();
		new_kb.push(Vec::new());
		new_kb
	};
	
	//is there enough space on the keyboard
	let to_add: Vec<String> = args.split_whitespace().map(|x| x.to_string()).collect();
	let used: usize = keyboard.iter().map(|row| row.len()).sum();
	let free = CAPACITY - used;
	if free < to_add.len() {
		return Err(Error::NotEnoughSpace(free).into());
	}

	//add buttons to the end of the keyboard
	let mut row = keyboard.last_mut().unwrap();
	for button in to_add {
		if row.len() == MAX_COLUMN {
			keyboard.push(Vec::new());
			row = keyboard.last_mut().unwrap();
		}
		row.push(button.into());
	}

	//store new keyboard
    let keyboard_json = serde_json::to_string(&keyboard).unwrap();
	user.keyboard = Some(keyboard_json);

	state.user_db
		.set_user(user.clone()).await
		.map_err(|e| Error::DbError(e))?;

	//update users keyboard
	reload(chat_id, token, user, "updated keyboard").await?;
    Ok(())
}


pub async fn remove_button(chat_id: ChatId, state: &DataRouterState, token: &str, 
    args: String, mut user: User)
     -> Result<(), botError> {

	//load keyboard
	let keyboard_str = user.keyboard.ok_or(Error::NoKeyboardSet)?;
	let keyboard: Keyboard = serde_json::from_str(&keyboard_str).unwrap();
	
	//flattern and recreate keyboard without the to be removed keys
	let to_remove: HashSet<String> = args.split_whitespace().map(|s|s.to_string()).collect();
	let keyboard: Keyboard = keyboard
		.into_iter()
		.flatten()
		.filter(|button| !to_remove.contains(button))
		.chunks(MAX_COLUMN)
			.into_iter()
			.map(|chunk| chunk.collect())
		.collect();

	//store new keyboard
    let keyboard_json = serde_json::to_string(&keyboard).unwrap();
	user.keyboard = Some(keyboard_json);

	state.user_db
		.set_user(user.clone()).await
		.map_err(|e| Error::DbError(e))?;

	//update users keyboard
	reload(chat_id, token, user, "updated keyboard").await?;
    Ok(())
}