use dialoguer::Select;
use crate::Connection;

mod user;
mod data;

fn show_menu() -> usize {
	Select::new()
		.paged(true)
		.item("shutdown")
		.item("modify/remove users")
		.item("export/archive datasets")
		.item("add user")
		.item("add dataset")
		.default(0)
		.interact()
		.unwrap()
}

pub fn command_line_interface(mut conn: Connection) {
	loop {
		match show_menu() {
			0 => break,
			1 => user::menu(&mut conn),
			2 => data::choose_dataset(&mut conn),
			3 => user::add_user(&mut conn),
			4 => data::add_set(&mut conn),
			_ => panic!(),
		}
	}
}
