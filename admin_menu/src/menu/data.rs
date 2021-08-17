use std::thread;
use std::time::Duration;

use dialoguer::{Input, Select};
use crate::Connection;
use dataserver::DataSetId;

pub fn add_set(conn: &mut Connection) {
    conn.ensure_template();

	let file_name = loop {
        let mut paths = conn.get_specs();

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

	match conn.add_dataset(&file_name) {
		Ok(dataset_id) => println!("Added dataset, id:{}", dataset_id),
		Err(e) => println!("could not create new dataset, error: {:?}", e),
	}
	thread::sleep(Duration::from_secs(2))
}

pub fn choose_dataset(conn: &mut Connection) {
	let dataset_list = conn.get_dataset_lists();

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
	modify_set(set_id, conn);
}

fn modify_set(set_id: DataSetId, conn: &mut Connection) {
	let metadata = conn.get_metadata(set_id);

	println!(
		"name: {:?}\ndescription: {:?}\nsecret key: {:?}\nset id:{:?}",
		metadata.name, metadata.description, metadata.key, set_id
	);
	print!("fields: ");
    for field in conn.dataset_fields(set_id) {
        print!("{}: {}, ", field.id, field.name)
    }
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
		3 => unimplemented!(),
		4 => conn.export(set_id),
		_ => unreachable!(),
	}
}
