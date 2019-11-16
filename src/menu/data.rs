use std::sync::{Arc, RwLock};
use std::path::Path;
use std::fs;
use std::thread;
use std::time::Duration;
use std::collections::HashSet;

use dialoguer::{Select, Input, PasswordInput, Checkboxes};
use telegram_bot::types::refs::UserId as TelegramUserId;
use log::{info, error};

use crate::databases::{PasswordDatabase, WebUserDatabase, BotUserDatabase, WebUserInfo, BotUserInfo};
use crate::data_store::{Data, DatasetId, read_to_array};
use crate::data_store;
use crate::error::DataserverError as Error;

pub fn add_set(data: &Arc<RwLock<Data>>) {
    
	if !Path::new("specs/template.yaml").exists() {
		data_store::specifications::write_template().unwrap();
	}
	if !Path::new("specs/template_for_test.yaml").exists() {
		data_store::specifications::write_template_for_test().unwrap();
	}

    let file_name = loop {
        let mut paths: Vec<String> = fs::read_dir("specs").unwrap()
            .filter_map(|dir_entry| dir_entry.ok())
            .map(|dir_entry| dir_entry.path().into_os_string())
            .filter_map(|path| path.into_string().ok())
            .filter(|path| path.ends_with("yaml"))
            .collect();

        println!("choose spec for new dataset");
        let list_numb = Select::new()
            .paged(true)
            .item("back")
            .item("refresh")
            .item("enter name")
            .items(&paths)
            .default(1)
            .interact().unwrap();

        match list_numb {
            0 => return,
            1 => continue,
            2 => break Input::<String>::new()
                    .with_prompt("Enter path to specification")
                    .interact()
                    .unwrap(),
            _ => break paths.remove(list_numb-2),
        }
    };

	let mut data = data.write().unwrap();
	match data.add_set(file_name){
        Ok(dataset_id) => println!("Added dataset, id:{}", dataset_id),
	    Err(e) => println!("could not create new dataset, error: {:?}", e),
    }
    thread::sleep(Duration::from_secs(2))
}

pub fn choose_dataset(user_db: &mut WebUserDatabase, bot_db: &mut BotUserDatabase, 
    data: &Arc<RwLock<Data>>) {
    
    let dataset_list: (Vec<String>, Vec<DatasetId>) = data.read()
        .unwrap().sets
        .iter()
        .map(|(id, dataset)| 
            (format!("{}: {}", id,dataset.metadata.name), id) 
        ).unzip();

    let list_numb = Select::new()
        .paged(true)
        .item("back")
        .items(&dataset_list.0)
        .default(0)
        .interact().unwrap();
    
    if list_numb == 0 {return;}        

    let index = list_numb-1;
    let set_id = dataset_list.1[index as usize];
    modify_set(set_id, user_db, bot_db, data);
}

fn modify_set(set_id: DatasetId, user_db: &mut WebUserDatabase, 
    bot_db: &mut BotUserDatabase, data: &Arc<RwLock<Data>>) {

    let metadata = data.read()
        .unwrap().sets
        .get(&set_id).unwrap()
        .metadata.clone();
    
    println!("name: {:?}\ndescription: {:?}\nsecret key: {:?}", 
        metadata.name, 
        metadata.description,
        metadata.key);
    print!("fields: ");
    metadata.fields.iter().for_each(|field| print!("{}: {}, ", field.id, field.name));
    println!("");


    let list_numb = Select::new()
        .paged(true)
        .item("back")
        .item("archive dataset")
        .item("export dataset")
        .default(0)
        .interact().unwrap();
    
    match list_numb {
        0 => return,
        1 => archive(set_id, user_db, bot_db, data),
        2 => export(set_id, data),
        _ => unreachable!(),
    }
}

fn export(set_id: DatasetId, data: &Arc<RwLock<Data>>){
    //let (x_shared, y_datas) = read_into_arrays(data, reader_info);
    unimplemented!();
}

fn archive(set_id: DatasetId, user_db: &mut WebUserDatabase, 
    bot_db: &mut BotUserDatabase, data: &Arc<RwLock<Data>> ) {

    //remove all mentions of set in all databases
    for mut userinfo in user_db.storage.iter().keys()
        .filter_map(Result::ok)
        .map(|username_bytes| user_db.get_userdata(&username_bytes))
        .filter_map(Result::ok){

        //remove set from this users timeseries with access
        if userinfo.timeseries_with_access.remove(&set_id).is_some(){
            //if there is a telegram bot remove the set from the botdb too
            if let Some(id) = userinfo.telegram_user_id{
                let mut botuserinfo = bot_db.get_userdata(id).unwrap();
                botuserinfo.timeseries_with_access.remove(&set_id);
                bot_db.set_userdata(id, botuserinfo).unwrap();
            }
        }
        user_db.set_userdata(userinfo).unwrap();
    }

    //remove from data HashMap
    data.write().unwrap().sets.remove(&set_id);

    //archive on filesystem
    let data_dir = data.read().unwrap().dir.clone();
    let mut archive_dir = data_dir.clone();
    archive_dir.push("archive");

    //we want to ignore errors here....
    if fs::create_dir(&archive_dir).is_ok() {
        info!("Created archive directory: {:?}", archive_dir);
    };

    let mut org_location = data_dir.clone();
    let mut new_location = archive_dir.clone();
    org_location.push(format!("{}",set_id));
    new_location.push(format!("{}",set_id));

    for extension in ["h","dat","yaml"].iter() {
        org_location.set_extension(extension);
        new_location.set_extension(extension);
        if let Err(e) = fs::rename(&org_location, &new_location){
            error!("could not move file {:?} to {:?}", org_location, new_location);
        };
    }
}