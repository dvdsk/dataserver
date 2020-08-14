use std::collections::HashSet;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;

use dialoguer::{Input, MultiSelect, Password, Select};
use futures::{executor::block_on, future};

use crate::data_store::{Authorisation, Data, DatasetId};
use crate::databases::{Access, AlarmDatabase, PasswordDatabase, User, UserDatabase, UserLookup};
use crate::error::DataserverError as Error;
use bitspec::{FieldId, MetaData};

pub fn menu(
	mut user_db: &mut UserDatabase,
	lookup: &UserLookup,
	passw_db: &mut PasswordDatabase,
	alarm_db: &AlarmDatabase,
	data: &Arc<RwLock<Data>>,
) {
	let (userlist, user_ids): (Vec<String>, Vec<u64>) = lookup
		.name_to_id
		.read()
		.unwrap()
		.iter()
		.map(|(s, id)| (s.into(), id))
		.unzip();

	let list_numb = Select::new()
		.paged(true)
		.item("back")
		.items(&userlist)
		.default(0)
		.interact()
		.unwrap();
	if list_numb == 0 {
		return;
	}

	let user_id = user_ids[list_numb - 1];
	let mut user = user_db.get_user(user_id).unwrap();
	let org_user = user.clone();

	loop {
		let numb_datasets = user.timeseries_with_access.len();

		println!("user: {}", user.name);
		println!("  last login: \t{}", user.last_login);
		println!("  telegram_id: \t{:?}\n", user.telegram_id);

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
			.interact()
			.unwrap();

		match list_numb {
			0 => change_user_name(&mut user, &lookup),
			1 => set_telegram_id(&mut user, &lookup),
			2 => change_dataset_access(&mut user, &data),
			3 => change_password(&user.name, passw_db).unwrap(),
			4 => {
				block_on(remove_user(user, user_db, passw_db, alarm_db, lookup)).unwrap();
				break;
			}
			5 => break,
			6 => {
				block_on(save_changes(
					&mut user_db,
					passw_db,
					user,
					org_user,
					&lookup,
				))
				.unwrap();
				break;
			}
			_ => panic!(),
		}
	}
}

fn change_password(name: &str, passw_db: &mut PasswordDatabase) -> Result<(), Error> {
	let new_password = Password::new()
		.with_prompt("New Password")
		.with_confirmation("Confirm password", "Passwords mismatching")
		.interact()
		.unwrap();

	println!("updating password please wait");
	passw_db.set_password(name.as_bytes(), new_password.as_bytes())?;
	Ok(())
}

pub fn add_user(user_db: &mut UserDatabase, passw_db: &mut PasswordDatabase, lookup: &UserLookup) {
	let name = Input::<String>::new()
		.with_prompt("Enter username (leave empty to abort)")
		.interact()
		.unwrap();

	if name.len() == 0 {
		println!("name must be at least 1 character");
		thread::sleep(Duration::from_secs(2));
		return;
	}

	if user_db.storage.contains_key(&name).unwrap() {
		println!("cant use \"{}\" as name, already in use", name);
		thread::sleep(Duration::from_secs(1));
		return;
	}

	if let Ok(password) = Password::new()
		.with_prompt("Enter password")
		.with_confirmation("Confirm password", "Passwords mismatching")
		.interact()
	{
		println!("setting password please wait");
		passw_db
			.set_password(name.as_bytes(), password.as_bytes())
			.unwrap();
		let id = block_on(user_db.new_user(name.clone())).unwrap();
		lookup.add(name, id);
	}
}

async fn remove_user(
	user: User,
	user_db: &mut UserDatabase,
	passw_db: &PasswordDatabase,
	alarm_db: &AlarmDatabase,
	lookup: &UserLookup,
) -> Result<(), Error> {
	lookup.remove_by_name(&user.name);
	let res = future::join3(
		passw_db.remove_user(user.name.as_str().as_bytes()),
		alarm_db.remove_user(user.id),
		user_db.remove_user(user.id),
	)
	.await;

	res.2?;
	res.1?;
	res.0?;
	Ok(())
}

async fn save_changes(
	user_db: &mut UserDatabase,
	passw_db: &mut PasswordDatabase,
	user: User,
	org_user: User,
	lookup: &UserLookup,
) -> Result<(), Error> {
	lookup.update(&org_user, &user);
	passw_db.update(&org_user.name, &user.name);

	user_db.set_user(user).await.unwrap();
	Ok(())
}

fn change_user_name(user: &mut User, lookup: &UserLookup) {
	let new_name = Input::<String>::new()
		.with_prompt("Enter new username")
		.allow_empty(true)
		.interact()
		.unwrap();

	if new_name.len() > 0 {
		if lookup.is_unique_name(&new_name) {
			user.name = new_name;
		} else {
			println!("cant use \"{}\" as name, already in use", new_name);
			thread::sleep(Duration::from_secs(1));
		}
	} else {
		println!("name must be at least 1 character");
		thread::sleep(Duration::from_secs(1));
	}
}

fn set_telegram_id(user: &mut User, lookup: &UserLookup) {
	let new_id = Input::<String>::new()
		.with_prompt("Enter new telegram id, leave empty to cancel")
		.allow_empty(true)
		.interact()
		.unwrap();

	if new_id.len() > 0 {
		if let Ok(new_id) = new_id.parse::<i64>() {
			if lookup.is_unique_telegram_id(&new_id.into()) {
				user.telegram_id.replace(new_id.into());
			} else {
				println!("TelegramId already in use!");
				thread::sleep(Duration::from_secs(1));
			}
		} else {
			println!("Can not parse to integer, please try again");
			thread::sleep(Duration::from_secs(1));
		}
	} else {
		user.telegram_id.take();
		println!("unset telegram id");
		thread::sleep(Duration::from_secs(1));
	}
}

fn change_dataset_access(user: &mut User, data: &Arc<RwLock<Data>>) {
	let access = &mut user.timeseries_with_access;
	let data_unlocked = data.read().unwrap();
	let dataset_list: (Vec<String>, Vec<DatasetId>) = access
		.iter()
		.map(|(id, _authorizations)| {
			let name = &data_unlocked.sets.get(id).unwrap().metadata.name;
			(format!("modify access to: {}", name), id)
		})
		.unzip();

	let list_numb = Select::new()
		.paged(true)
		.item("back")
		.item("add dataset")
		.items(&dataset_list.0)
		.default(0)
		.interact()
		.unwrap();

	match list_numb {
		0 => return,
		1 => add_dataset(data, access),
		_ => {
			let set_id = dataset_list.1[list_numb - 2];
			modify_dataset_fields(set_id, access, data);
		}
	}
}

fn add_dataset(data: &Arc<RwLock<Data>>, access: &mut Access) {
	let dataset_list: (Vec<String>, Vec<DatasetId>) = data
		.read()
		.unwrap()
		.sets
		.iter()
		.filter(|(id, _)| !access.contains_key(&id))
		.map(|(id, dataset)| (format!("{}: {}", id, dataset.metadata.name), id))
		.unzip();

	println!("choose a dataset");
	let list_numb = Select::new()
		.paged(true)
		.item("back")
		.item("select dataset with set_id")
		.items(&dataset_list.0)
		.default(0)
		.interact()
		.unwrap();

	let set_id = match list_numb {
		0 => {
			return;
		}
		1 => {
			if let Ok(set_id) = Input::<String>::new()
				.with_prompt("Enter dataset id")
				.interact()
				.unwrap()
				.parse::<DatasetId>()
			{
				set_id
			} else {
				return;
			}
		}
		_ => dataset_list.1[list_numb - 2],
	};

	let authorized_fields = select_fields(set_id, data);
	access.insert(set_id, authorized_fields);
}

fn select_fields(set_id: DatasetId, data: &Arc<RwLock<Data>>) -> Vec<Authorisation> {
	let mut field_list: (Vec<String>, Vec<FieldId>) = data
		.read()
		.unwrap()
		.sets
		.get(&set_id)
		.unwrap()
		.metadata
		.fields
		.iter()
		.map(|field| (format!("{}", field.name), field.id))
		.unzip();

	let list_numbs = MultiSelect::new()
		.with_prompt("select fields to add as owner")
		.paged(true)
		.items(&field_list.0)
		.interact()
		.unwrap();

	let mut authorized_fields: Vec<Authorisation> = list_numbs
		.iter()
		.map(|index| {
			let field_id = field_list.1[*index];
			Authorisation::Owner(field_id)
		})
		.collect();
	authorized_fields.sort_unstable();

	//remove chosen items from possible reader fields
	let mut counter = 0;
	list_numbs.iter().for_each(|list_numb| {
		field_list.0.remove(list_numb - counter);
		counter += 1;
	});

	if !field_list.0.is_empty() {
		let list_numbs = MultiSelect::new()
			.with_prompt("select fields to add as reader")
			.paged(true)
			.items(&field_list.0)
			.interact()
			.unwrap();

		list_numbs
			.iter()
			.map(|index| {
				let field_id = field_list.1[*index];
				Authorisation::Reader(field_id)
			})
			.for_each(|auth| authorized_fields.push(auth));
	}

	authorized_fields
}

fn make_field_actions(
	metadata: &MetaData,
	accessible_fields: &HashSet<Authorisation>,
) -> (Vec<String>, Vec<Authorisation>, Vec<String>, Vec<FieldId>) {
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

fn modify_dataset_fields(set_id: DatasetId, access: &mut Access, data: &Arc<RwLock<Data>>) {
	let fields_with_access = access.get_mut(&set_id);
	if fields_with_access.is_none() {
		return;
	}
	let fields_with_access = fields_with_access.unwrap();
	let mut accessible_fields: HashSet<Authorisation> = fields_with_access.drain(..).collect();

	let metadata = &data
		.read()
		.unwrap()
		.sets
		.get(&set_id)
		.unwrap()
		.metadata
		.clone();

	while accessible_fields.len() > 0 {
		let (removable, removable_access, addable, addable_ids) =
			make_field_actions(metadata, &accessible_fields);
		let list_numb = Select::new()
			.paged(true)
			.item("back")
			.items(&removable)
			.items(&addable)
			.default(0)
			.interact()
			.unwrap();

		if list_numb == 0 {
			accessible_fields
				.drain()
				.for_each(|auth| fields_with_access.push(auth));
			return;
		}

		if list_numb - 1 < removable.len() {
			dbg!(list_numb);
			let access = &removable_access[list_numb - 1 as usize];
			accessible_fields.take(access);
		} else {
			let id = addable_ids[list_numb - 1 - removable.len() as usize];
			let list_numb = Select::new()
				.item("back")
				.item("add as reader")
				.item("add as owner")
				.default(0)
				.interact()
				.unwrap();

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
