use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;
use std::collections::{HashSet, HashMap};
use chrono::Utc;

use dialoguer::{Select, Input, PasswordInput, Checkboxes};
use telegram_bot::types::refs::UserId as TelegramUserId;

use crate::databases::{PasswordDatabase, WebUserDatabase, BotUserDatabase, WebUserInfo, BotUserInfo, Access};
use crate::data_store::{Data, Authorisation, DatasetId, FieldId, MetaData};
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
    let mut botuserinfo = userinfo.telegram_user_id.map(|id| bot_db.get_userdata(id).unwrap());
    let org_userinfo = userinfo.clone();
    let org_botuserinfo = botuserinfo.clone();
    
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
            0 => change_user_name(&mut userinfo, &mut botuserinfo, user_db),
            1 => set_telegram_id(&mut userinfo, &mut botuserinfo),
            2 => change_dataset_access(&mut userinfo, &mut botuserinfo, &data),
            3 => change_password(username, passw_db).unwrap(),
            4 => {remove_user(userinfo, user_db, bot_db, passw_db).unwrap(); break;}
            5 => break,
            6 => {save_changes(user_db, bot_db,
                userinfo, org_userinfo, 
                botuserinfo, org_botuserinfo).unwrap(); 
                break;},
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

pub fn add_user(user_db: &mut WebUserDatabase, passw_db: &mut PasswordDatabase){
    let username = Input::<String>::new()
    .with_prompt("Enter username (leave empty to abort)")
    .interact()
    .unwrap();

    if username.len() == 0 {
        println!("name must be at least 1 character");
        thread::sleep(Duration::from_secs(2));
        return;
    } 
    
    if user_db.storage.contains_key(&username).unwrap() {
        println!("cant use \"{}\" as name, already in use", username);    
        thread::sleep(Duration::from_secs(1));
        return;  
    }

	let user_data = WebUserInfo{
		timeseries_with_access: HashMap::new(),
		last_login: Utc::now(),
		username: username.clone(),
		telegram_user_id: None,
	};

    if let Ok(password) = PasswordInput::new().with_prompt("Enter password")
        .with_confirmation("Confirm password", "Passwords mismatching")
        .interact(){

        println!("setting password please wait");
        passw_db.set_password(username.as_bytes(), password.as_bytes()).unwrap();
        user_db.set_userdata(user_data).unwrap();       
    }
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

fn save_changes(user_db: &WebUserDatabase, bot_db: &mut BotUserDatabase,
                userinfo: WebUserInfo, org_userinfo: WebUserInfo, 
                botuser: Option<BotUserInfo>, org_botuser: Option<BotUserInfo>)
                 -> Result<(), Error> {

    //remove what should be removed
    if org_userinfo.username != userinfo.username {
        user_db.remove_user(org_userinfo.username).unwrap();
    }
    if org_userinfo.telegram_user_id != userinfo.telegram_user_id {
        if let Some(telegram_user_id) = org_userinfo.telegram_user_id{
            bot_db.remove_user(telegram_user_id).unwrap();
        }
    }

    //insert everything what is not None
    if let Some(botuser) = botuser {
        let id = userinfo.telegram_user_id.unwrap();
        bot_db.set_userdata(id, &botuser)?;
    }

    user_db.set_userdata(userinfo).unwrap();
    Ok(())
}

fn change_user_name(userinfo: &mut WebUserInfo, botuserinfo: &mut Option<BotUserInfo>, user_db: &WebUserDatabase) {
    let new_name = Input::<String>::new()
    .with_prompt("Enter new username")
    .interact()
    .unwrap();

    if new_name.len() > 0 {
        if !user_db.storage.contains_key(&new_name).unwrap() {
            botuserinfo.as_mut().map(|mut u| u.username=new_name.clone()); 
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

fn set_telegram_id(userinfo: &mut WebUserInfo, botuser: &mut Option<BotUserInfo>) {
    let new_id = Input::<String>::new()
        .with_prompt("Enter new telegram id")
        .interact()
        .unwrap();
    
    if new_id.len() > 0 {
        if let Ok(new_id) = new_id.parse::<i64>(){
            if botuser.is_none(){
                let access = userinfo.timeseries_with_access.clone();
                *botuser = Some(BotUserInfo::from_access_and_name(access, userinfo.username.clone()));
            }
            userinfo.telegram_user_id = Some(TelegramUserId::new(new_id));
        } else {
            println!("Can not parse to integer, please try again");            
            thread::sleep(Duration::from_secs(1));   
        }
    } else {
        println!("unset telegram id");
        *botuser = None;
        userinfo.telegram_user_id = None;
        thread::sleep(Duration::from_secs(1));
    }
}

fn change_dataset_access(userinfo: &mut WebUserInfo, botuser: &mut Option<BotUserInfo>, 
    data: &Arc<RwLock<Data>>) {
    
    let access = &mut userinfo.timeseries_with_access;
    let data_unlocked = data.read().unwrap();
    let dataset_list: (Vec<String>, Vec<DatasetId>) 
    = access.iter()
        .map(|(id, _authorizations)| {
            let name = &data_unlocked.sets.get(id).unwrap().metadata.name;
            (format!("modify access to: {}", name), id)})
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
        1 => add_dataset(data, access),
        _ => {
            let set_id = dataset_list.1[list_numb - 2]; 
            modify_dataset_fields(set_id, access, data);
        }
    }

    botuser.as_mut().map(|mut b| b.timeseries_with_access = access.clone());
}

fn add_dataset(data: &Arc<RwLock<Data>>, access: &mut Access){
    let dataset_list: (Vec<String>, Vec<DatasetId>) = data.read()
        .unwrap().sets
        .iter()
        .filter(|(id, _)| !access.contains_key(&id))
        .map(|(id, dataset)| 
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
        0 => {return;},
        1 => if let Ok(set_id) = Input::<String>::new()
                .with_prompt("Enter dataset id").interact()
                .unwrap().parse::<DatasetId>(){
                
                set_id
            } else {return;},
        _ => dataset_list.1[list_numb-2],
    };

    let authorized_fields = select_fields(set_id, data);
    access.insert(set_id, authorized_fields);

}

fn select_fields(set_id: DatasetId, data: &Arc<RwLock<Data>>)
    -> Vec<Authorisation> {
    let mut field_list: (Vec<String>, Vec<FieldId>) = data.read()
        .unwrap().sets
        .get(&set_id).unwrap()
        .metadata.fields.iter()
        .map(|field| (format!("{}", field.name),field.id))
        .unzip();

    let list_numbs = Checkboxes::new()
        .with_prompt("select fields to add as owner")
        .paged(true)
        .items(&field_list.0)
        .interact().unwrap();
    
    let mut authorized_fields: Vec<Authorisation> = list_numbs.iter().map(|index| {
        let field_id = field_list.1[*index];
        Authorisation::Owner(field_id)
    }).collect();
    authorized_fields.sort_unstable();

    //remove chosen items from possible reader fields
    let mut counter = 0;
    list_numbs.iter().for_each(|list_numb| {
        field_list.0.remove(list_numb-counter);
        counter+=1;
    });

    if !field_list.0.is_empty() {
        let list_numbs = Checkboxes::new()
            .with_prompt("select fields to add as reader")
            .paged(true)
            .items(&field_list.0)
            .interact().unwrap();

    list_numbs.iter().map(|index| {
        let field_id = field_list.1[*index];
        Authorisation::Reader(field_id)
    }).for_each(|auth| authorized_fields.push(auth));
    }

    authorized_fields
}

fn make_field_actions(metadata: &MetaData, accessible_fields: &HashSet<Authorisation>)
-> (Vec<String>, Vec<Authorisation>,Vec<String>, Vec<FieldId>) {
    let mut removable = Vec::with_capacity(metadata.fields.len());
    let mut removable_ids = Vec::with_capacity(metadata.fields.len());

    let mut addable = Vec::with_capacity(metadata.fields.len());
    let mut addable_ids = Vec::with_capacity(metadata.fields.len());

    for field in &metadata.fields {
        if accessible_fields.contains(&Authorisation::Owner(field.id)) {
            removable.push(format!("remove owned field: {}", field.name));
            removable_ids.push(Authorisation::Owner(field.id));
        } else if accessible_fields.contains(&Authorisation::Reader(field.id)) {
            removable.push(format!("remove reader field: {}", field.name));
            removable_ids.push(Authorisation::Reader(field.id));
        } else {
            addable.push(format!("add field: {}", field.name));
            addable_ids.push(field.id);
        }
    }

    (removable, removable_ids, addable, addable_ids)
}

fn modify_dataset_fields(set_id: DatasetId, access: &mut Access, data: &Arc<RwLock<Data>>){
    let fields_with_access = access.get_mut(&set_id);
    if fields_with_access.is_none() {return; }
    let fields_with_access = fields_with_access.unwrap();
    let mut accessible_fields: HashSet<Authorisation> = fields_with_access
        .drain(..)
        .collect();

    let metadata = &data.read()
        .unwrap().sets
        .get(&set_id).unwrap()
        .metadata.clone();

    while accessible_fields.len() > 0 {
        let (removable, removable_access, addable, addable_ids) = make_field_actions(metadata, &accessible_fields);
        let list_numb = Select::new()
            .paged(true)
            .item("back")
            .items(&removable)
            .items(&addable)
            .default(0)
            .interact().unwrap();

        if list_numb == 0 {
            accessible_fields.drain()
                .for_each(|auth| fields_with_access.push(auth));
            return;
        }
        
        if list_numb-1 < removable.len() {
            dbg!(list_numb);
            let access = &removable_access[list_numb-1 as usize];
            accessible_fields.take(access);
        } else {
            let id = addable_ids[list_numb-1-removable.len() as usize];
            let list_numb = Select::new()
            .item("back")
            .item("add as reader")
            .item("add as owner")
            .default(0)
            .interact().unwrap();

            match list_numb {
                0 => continue,
                1 => accessible_fields.insert(Authorisation::Reader(id)),
                2 => accessible_fields.insert(Authorisation::Owner(id)),
                _ => unreachable!(),
            };
        }
    }

    //no more fields with access left remove dataset from acessible sets
    access.remove(&set_id);
}