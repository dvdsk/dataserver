use std::thread;
use std::time::Duration;
use crate::Connection;
use dialoguer::{Input, MultiSelect, Password, Select};
use std::collections::HashSet;

use dataserver::{UserId, DataSetId, Authorisation, bitspec};

pub fn menu(conn: &mut Connection) {
	let (userlist, user_ids): (Vec<String>, Vec<u64>) = conn.get_user_lists();

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
	let user = conn.get_user_by_id(user_id);

	loop {
		println!("user: {}", user.name);
		println!("  last login: \t{}", user.last_login);
		println!("  telegram_id: \t{:?}\n", user.telegram_id);

		let list_numb = Select::new()
			.paged(true)
			.item("change name")
			.item("change telegram id")
			.item("change dataset access")
			.item("change password")
			.item("remove user")
			.item("back")
			.default(3)
			.interact()
			.unwrap();

		match list_numb {
			0 => change_user_name(user_id, conn),
			1 => set_telegram_id(user_id, conn),
			2 => change_dataset_access(user_id, conn),
			3 => change_password(user_id, conn),
			4 => {conn.remove_user(user_id); break},
			5 => break,
			_ => panic!(),
		}
	}
}

fn change_password(user_id: UserId, conn: &mut Connection) {
	let new_password = Password::new()
		.with_prompt("New Password")
		.with_confirmation("Confirm password", "Passwords mismatching")
		.interact()
		.unwrap();

	println!("updating password please wait");
	conn.set_password(user_id, &new_password);
}

pub fn add_user(conn: &mut Connection) {
	let name = Input::<String>::new()
		.with_prompt("Enter username (leave empty to abort)")
		.interact()
		.unwrap();

	let password = Password::new()
		.with_prompt("Enter password")
		.with_confirmation("Confirm password", "Passwords mismatching")
		.interact()
        .unwrap();


    if let Err(e) = conn.add_user(&name, &password) {
        println!("could not add new user: {}", e);
		thread::sleep(Duration::from_secs(1));
    }
}

fn change_user_name(user_id: UserId, conn: &mut Connection) {
	let new_name = Input::<String>::new()
		.with_prompt("Enter new username")
		.allow_empty(true)
		.interact()
		.unwrap();

    if let Err(e) = conn.change_user_name(user_id, &new_name){
        println!("could not set new username: {}", e);
		thread::sleep(Duration::from_secs(1));
    }
}

fn set_telegram_id(user_id: UserId, conn: &mut Connection) {
	let new_id = Input::<String>::new()
		.with_prompt("Enter new telegram id, leave empty to cancel")
		.allow_empty(true)
		.interact()
		.unwrap();

    if let Err(e) = conn.change_telegram_id(user_id, &new_id){
        println!("could not set telegram id: {}", e);
		thread::sleep(Duration::from_secs(1));
    }
}

fn change_dataset_access(user_id: UserId, conn: &mut Connection) {
	let dataset_lists: (Vec<String>, Vec<DataSetId>) = conn.acessible_datasets(user_id);

	let list_numb = Select::new()
		.paged(true)
		.item("back")
		.item("add dataset")
		.items(&dataset_lists.0)
		.default(0)
		.interact()
		.unwrap();

	match list_numb {
		0 => (),
		1 => add_dataset(user_id, conn),
		_ => {
			let set_id = dataset_lists.1[list_numb - 2];
			modify_dataset_fields(set_id, user_id, conn);
		}
	}
}

fn add_dataset(user_id: UserId, conn: &mut Connection) {
	let dataset_list: (Vec<String>, Vec<DataSetId>) = conn.inaccesible_datsets();

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
				.parse::<DataSetId>()
			{
				set_id
			} else {
				return;
			}
		}
		_ => dataset_list.1[list_numb - 2],
	};

	let authorized_fields = select_fields(set_id, conn);
	conn.make_fields_accesible(set_id, authorized_fields);
}

fn select_fields(set_id: DataSetId, conn: &mut Connection) -> Vec<Authorisation> {
    let mut fields = conn.dataset_fields(set_id);
    let (mut names, ids): (Vec<_>, Vec<_>) = fields.drain(..).map(|f| (f.name, f.id)).unzip();

	let list_numbs = MultiSelect::new()
		.with_prompt("select fields to add as owner")
		.paged(true)
		.items(&names)
		.interact()
		.unwrap();

	let mut authorized_fields: Vec<Authorisation> = list_numbs
		.iter()
		.map(|index| {
			let field_id = ids[*index];
			Authorisation::Owner(field_id)
		})
		.collect();
	authorized_fields.sort_unstable();

	//remove chosen items from possible reader fields
	let mut counter = 0;
	list_numbs.iter().for_each(|list_numb| {
		names.remove(list_numb - counter);
		counter += 1;
	});

	if !names.is_empty() {
		let list_numbs = MultiSelect::new()
			.with_prompt("select fields to add as reader")
			.paged(true)
			.items(&names)
			.interact()
			.unwrap();

		list_numbs
			.iter()
			.map(|index| {
				let field_id = ids[*index];
				Authorisation::Reader(field_id)
			})
			.for_each(|auth| authorized_fields.push(auth));
	}

	authorized_fields
}

fn make_field_actions(
	metadata: &bitspec::FixedLine,
	accessible_fields: &HashSet<Authorisation>,
) -> (Vec<String>, Vec<Authorisation>, Vec<String>, Vec<bitspec::FieldId>) {
	let mut removable = Vec::new();
	let mut removable_ids = Vec::new();

	let mut addable = Vec::new();
	let mut addable_ids = Vec::new();

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

fn modify_dataset_fields(set_id: DataSetId, user_id: UserId, conn: &mut Connection) {
	let mut fields_with_access = conn.accessible_fields(set_id);
	if fields_with_access.is_empty() {
		return;
	}
	let mut accessible_fields: HashSet<_> = fields_with_access.drain(..).collect();

	let metadata = conn.get_metadata(set_id);
	while !accessible_fields.is_empty() {
		let (removable, removable_access, addable, addable_ids) =
			make_field_actions(&metadata, &accessible_fields);
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
	conn.remove_dataset_access(set_id);
}
