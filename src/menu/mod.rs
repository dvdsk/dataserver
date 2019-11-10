use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;

use dialoguer::{Select, Input, PasswordInput};
use telegram_bot::types::refs::UserId as TelegramUserId;

use crate::databases::{PasswordDatabase, WebUserDatabase, BotUserDatabase, WebUserInfo, BotUserInfo};
use crate::data_store::{Data, Authorisation, DatasetId, FieldId};
use crate::error::DataserverError as Error;

mod user;

fn main_menu() -> usize {
    Select::new()
        .paged(true)
        .item("shutdown")
        .item("modify/remove users")
        .item("modify/remove datasets")
        .item("add user")
        .item("add dataset")
        .default(0)
        .interact().unwrap()
}

pub fn command_line_interface(data: Arc<RwLock<Data>>, 
                          mut passw_db: PasswordDatabase, 
                          mut web_user_db: WebUserDatabase,
                          mut bot_user_db: BotUserDatabase){

    loop {
        match main_menu(){
            0 => break,
            1 => user::menu(&mut web_user_db, &mut bot_user_db, 
                            &mut passw_db, &data),

            _ => panic!(),

        }
    }
}

// fn command_line_interface(data: Arc<RwLock<data_store::Data>>, 
//     mut passw_db: PasswordDatabase, 
//     mut web_user_db: WebUserDatabase,
//     mut bot_user_db: BotUserDatabase){
// loop {
// let mut input = String::new();
// stdin().read_line(&mut input).unwrap();
// match input.as_str() {
// "t\n" => helper::send_test_data_over_http(data.clone(), 8070),
// "n\n" => helper::add_user(&mut passw_db, &mut web_user_db),
// "a\n" => helper::add_dataset(&mut web_user_db, &data),
// "o\n" => helper::add_fields_to_user(&mut web_user_db, &mut bot_user_db),
// "q\n" => break,
// _ => println!("unhandled"),
// };
// }
// }