use std::fs;
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;
use actix_rt::net::TcpStream;

use dialoguer::{Input, Select};
use futures::executor::block_on;
use log::{error, info};

use crate::data_store::{Data, DatasetId};
use crate::database::UserDatabase;

pub fn add_set(stream: &mut TcpStream, data: &Arc<RwLock<Data>>) {
	if !Path::new("specs/template.yaml").exists() {
		bitspec::write_template().unwrap();
	}

	let file_name = loop {
		let mut paths: Vec<String> = fs::read_dir("specs")
			.unwrap()
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
			.interact()
			.unwrap();

		match list_numb {
			0 => return,
			1 => continue,
			2 => {
				break Input::<String>::new()
					.with_prompt("Enter path to specification")
					.interact()
					.unwrap()
			}
			_ => break paths.remove(list_numb - 3),
		}
	};

	let mut data = data.write().unwrap();
	match data.add_set(file_name) {
		Ok(dataset_id) => println!("Added dataset, id:{}", dataset_id),
		Err(e) => println!("could not create new dataset, error: {:?}", e),
	}
	thread::sleep(Duration::from_secs(2))
}

pub fn choose_dataset(stream: &mut TcpStream, user_db: &mut UserDatabase, data: &Arc<RwLock<Data>>) {
	let dataset_list: (Vec<String>, Vec<DatasetId>) = data
		.read()
		.unwrap()
		.sets
		.iter()
		.map(|(id, dataset)| (format!("{}: {}", id, dataset.metadata.name), id))
		.unzip();

	let list_numb = Select::new()
		.paged(true)
		.item("back")
		.items(&dataset_list.0)
		.default(0)
		.interact()
		.unwrap();

	if list_numb == 0 {
		return;
	}

	let index = list_numb - 1;
	let set_id = dataset_list.1[index as usize];
	modify_set(set_id, user_db, data);
}

fn modify_set(set_id: DatasetId, user_db: &mut UserDatabase, data: &Arc<RwLock<Data>>) {
	let metadata = data
		.read()
		.unwrap()
		.sets
		.get(&set_id)
		.unwrap()
		.metadata
		.clone();

	println!(
		"name: {:?}\ndescription: {:?}\nsecret key: {:?}\nset id:{:?}",
		metadata.name, metadata.description, metadata.key, set_id
	);
	print!("fields: ");
	metadata
		.fields
		.iter()
		.for_each(|field| print!("{}: {}, ", field.id, field.name));
	println!("\n");

	let list_numb = Select::new()
		.paged(true)
		.item("back")
		.item("change secret key")
		.item("change set id")
		.item("archive dataset")
		.item("export dataset")
		.default(0)
		.interact()
		.unwrap();

	match list_numb {
		0 => (),
		1 => unimplemented!(),
		2 => unimplemented!(),
		3 => archive(set_id, user_db, data),
		4 => export(set_id, data),
		_ => unreachable!(),
	}
}

fn export(_set_id: DatasetId, _data: &Arc<RwLock<Data>>) {
	//let (x_shared, y_datas) = read_into_arrays(data, reader_info);
	unimplemented!();
}

fn archive(set_id: DatasetId, user_db: &mut UserDatabase, data: &Arc<RwLock<Data>>) {
	//remove all mentions of set in all database
	for mut user in user_db.iter() {
		//remove access
		if user.timeseries_with_access.remove(&set_id).is_some() {
			//if user had access update userdata
			block_on(user_db.set_user(user)).unwrap();
		}
	}

	//remove from data HashMap
	data.write().unwrap().sets.remove(&set_id);

	//archive on filesystem
	let data_dir = data.read().unwrap().dir.clone();
	let mut archive_dir = data_dir.clone();
	archive_dir.push("archive");

	//we want to ignore errors here....
	//TODO filter errors
	if fs::create_dir(&archive_dir).is_ok() {
		info!("Created archive directory: {:?}", archive_dir);
	};

	let mut org_location = data_dir;
	let mut new_location = archive_dir;
	org_location.push(format!("{}", set_id));
	new_location.push(format!("{}", set_id));

	for extension in ["h", "dat", "yaml"].iter() {
		org_location.set_extension(extension);
		new_location.set_extension(extension);
		if let Err(e) = fs::rename(&org_location, &new_location) {
			error!(
				"could not move file {:?} to {:?}, cause: {:?}",
				org_location, new_location, e
			);
		};
	}
}
