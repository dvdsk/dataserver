use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;

use dialoguer::{Select, Input, PasswordInput, Checkboxes};
use telegram_bot::types::refs::UserId as TelegramUserId;

use crate::databases::{PasswordDatabase, WebUserDatabase, BotUserDatabase, WebUserInfo, BotUserInfo};
use crate::data_store::{Data, Authorisation, DatasetId, FieldId};
use crate::error::DataserverError as Error;

pub fn menu(user_db: &mut WebUserDatabase, bot_db: &mut BotUserDatabase, 
    passw_db: &mut PasswordDatabase, data: &Arc<RwLock<Data>>) {
    
    let userlist: Vec<String> = user_db.storage.iter().keys()
        .filter_map(Result::ok)
        .map(|username_bytes| String::from_utf8(username_bytes))
        .filter_map(Result::ok)
        .collect();

    let list_numb = Select::new()
        .paged(true)
        .item("back")
        .items(&userlist)
        .default(0)
        .interact().unwrap();
    if list_numb == 0 {return;}

    let username = userlist[list_numb - 1].as_str();
    let mut userinfo = user_db.get_userdata(&username).unwrap();
    let org_userinfo = userinfo.clone();
    
    loop {
        let numb_datasets = userinfo.timeseries_with_access.len();
        
        println!("user: {}", userinfo.username);
        println!("  last login: \t{}", userinfo.last_login);
        println!("  telegram_id: \t{:?}\n", userinfo.telegram_user_id);

        let list_numb = Select::new()
            .paged(true)
            .item("change name")
            .item("change telegram id")
            .item(&format!("change dataset access ({} sets)", numb_datasets))
            .item("change password")
            .item("remove user")
            .item("abort")
            .item("save and exit")
            .default(3)
            .interact().unwrap();
        
        match list_numb {
            0 => change_user_name(&mut userinfo, user_db),
            1 => set_telegram_id(&mut userinfo),
            2 => change_dataset_access(&mut userinfo, &data),
            3 => change_password(username, passw_db).unwrap(),
            4 => {remove_user(userinfo, user_db, bot_db, passw_db).unwrap(); break;}
            5 => break,
            6 => {save_changes(userinfo, user_db, org_userinfo, bot_db).unwrap(); break;},
            _ => panic!(),
        } 
    }
}

fn change_password(username: &str, passw_db: &mut PasswordDatabase) -> Result<(), Error> {
    let new_password = PasswordInput::new().with_prompt("New Password")
        .with_confirmation("Confirm password", "Passwords mismatching")
        .interact().unwrap();
    
    println!("updating password please wait");
    passw_db.set_password(username.as_bytes(), new_password.as_bytes())?;
    Ok(())
}

fn remove_user(userinfo: WebUserInfo, user_db: &WebUserDatabase, 
    bot_db: &mut BotUserDatabase, passw_db: &PasswordDatabase)
               -> Result<(), Error> {

    if let Some(id) = userinfo.telegram_user_id{
        bot_db.remove_user(id)?;
    }

    passw_db.remove_user(userinfo.username.as_str().as_bytes())?;
    user_db.remove_user(userinfo.username.as_str().as_bytes())?;
    Ok(())
}

fn save_changes(userinfo: WebUserInfo, user_db: &WebUserDatabase, org_userinfo: WebUserInfo, 
                bot_db: &mut BotUserDatabase) -> Result<(), Error> {
    if org_userinfo.username != userinfo.username {
        user_db.remove_user(org_userinfo.username).unwrap();
    }

    //if there was a telegram id set
    if let Some(org_id) = org_userinfo.telegram_user_id {
        if let Some(id) = userinfo.telegram_user_id {
            let botuserinfo = bot_db.get_userdata(org_id)?;
            bot_db.set_userdata(id, botuserinfo)?;
            bot_db.remove_user(org_id)?;
        } else {
            bot_db.remove_user(org_id)?;
        }
    } else {
        if let Some(id) = userinfo.telegram_user_id {
            let botuserinfo = BotUserInfo::from_timeseries_access(&userinfo.timeseries_with_access);
            bot_db.set_userdata(id, botuserinfo)?;
        }
        //do nothing as there was no telegram id anyway
    }

    user_db.set_userdata(userinfo).unwrap();
    Ok(())
}

fn change_user_name(userinfo: &mut WebUserInfo, user_db: &WebUserDatabase) {
    let new_name = Input::<String>::new()
    .with_prompt("Enter new username")
    .interact()
    .unwrap();

    if new_name.len() > 0 {
        if !user_db.storage.contains_key(&new_name).unwrap() {
            userinfo.username = new_name;
        } else {
            println!("cant use \"{}\" as name, already in use", new_name);    
            thread::sleep(Duration::from_secs(1));     
        }
    } else {
        println!("name must be at least 1 character");
        thread::sleep(Duration::from_secs(1));
    }
}

fn set_telegram_id(userinfo: &mut WebUserInfo) {
    let new_id = Input::<String>::new()
        .with_prompt("Enter new telegram id")
        .interact()
        .unwrap();
    
    if new_id.len() > 0 {
        if let Ok(new_id) = new_id.parse::<i64>(){
            userinfo.telegram_user_id = Some(TelegramUserId::new(new_id));
        } else {
            println!("Can not parse to integer, please try again");            
            thread::sleep(Duration::from_secs(1));   
        }
    } else {
        println!("unset telegram id");
        userinfo.telegram_user_id = None;
        thread::sleep(Duration::from_secs(1));
    }
}

fn change_dataset_access(userinfo: &mut WebUserInfo, data: &Arc<RwLock<Data>>) {
    let dataset_list: (Vec<String>, Vec<DatasetId>) 
    = userinfo.timeseries_with_access.iter()
        .map(|(id, _authorizations)| 
           (format!("modify access to: dataset {}", id), id))
        .unzip();

    let list_numb = Select::new()
        .paged(true)
        .item("back")
        .item("add dataset")
        .items(&dataset_list.0)
        .default(0)
        .interact().unwrap();

    match list_numb {
        0 => return,
        1 => add_dataset(data, userinfo),
        _ => {
            let set_id = dataset_list.1[list_numb - 2]; 
            modify_dataset_fields(set_id, userinfo);
        }
    }  
}


fn add_dataset(data: &Arc<RwLock<Data>>, userinfo: &mut WebUserInfo){
    let dataset_list: (Vec<String>, Vec<DatasetId>) = data.read()
        .unwrap().sets
        .iter().map(|(id, dataset)| 
            (format!("{}: {}",id,dataset.metadata.name), id) 
        ).unzip();
    
    println!("choose a dataset");
    let list_numb = Select::new()
        .paged(true)
        .item("back")
        .item("select dataset with set_id")
        .items(&dataset_list.0)
        .default(0)
        .interact().unwrap();    

    let set_id = match list_numb {
        0 => {return; 0},
        1 => if let Ok(set_id) = Input::<String>::new()
                .with_prompt("Enter dataset id").interact()
                .unwrap().parse::<DatasetId>(){
                
                set_id
            } else {return;},
        _ => dataset_list.1[list_numb-2],
    };

    let authorized_fields = select_fields(set_id, data);
    userinfo.timeseries_with_access.insert(set_id, authorized_fields);
}

fn select_fields(set_id: DatasetId, data: &Arc<RwLock<Data>>)
    -> Vec<Authorisation> {
    let field_list: (Vec<String>, Vec<FieldId>) = data.read()
        .unwrap().sets
        .get(&set_id).unwrap()
        .metadata.fields.iter()
        .map(|field| (format!("{}", field.name),field.id))
        .unzip();

    let list_numb = Checkboxes::new()
        .with_prompt("select fields to add as owner")
        .paged(true)
        .items(&field_list.0)
        .interact().unwrap();
    
    let authorized_fields = list_numb.iter().map(|index| {
        let field_id = field_list.1[*index];
        Authorisation::Owner(field_id)
    }).collect();

    authorized_fields
}

fn modify_dataset_fields(set_id: DatasetId, userinfo: &mut WebUserInfo){
    let fields = userinfo.timeseries_with_access.get_mut(&set_id);
    if fields.is_none() {return; }
    let fields = fields.unwrap();

    while fields.len() > 0 {
        let field_list: Vec<String> = fields.iter()
            .map(|field| {
                match field {
                    Authorisation::Owner(id) =>
                        format!("remove owned field: {}", id),
                    Authorisation::Reader(id) =>
                        format!("remove reading field: {}",id),
                }
            }).collect();

        let list_numb = Select::new()
            .paged(true)
            .item("back")
            .items(&field_list)
            .default(0)
            .interact().unwrap();

        if list_numb == 0 {return;}
        
        let index = list_numb -1;
        fields.remove(index);
    }
    userinfo.timeseries_with_access.remove(&set_id);
}