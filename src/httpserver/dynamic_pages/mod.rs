use actix_identity::Identity;
use actix_web::web::Data;
use actix_web::{HttpResponse, Responder};

extern crate yarte;
use yarte::Template;

use crate::bot::commands::show::format_to_duration;
use crate::data_store;
use data_store::{data_router::DataRouterState, Authorisation};

#[derive(Template)]
#[template(path = "settings.hbs")]
struct SettingsPage<'a> {
	telegram_id: &'a str,
}

pub async fn settings_page(_id: Identity, _state: Data<DataRouterState>) -> impl Responder {
	let page = SettingsPage {
		telegram_id: "test",
	};
	HttpResponse::Ok().body(page.call().unwrap())
}

#[derive(Default)]
struct ListSetInfo {
	name: String,
	last_updated: String,
	field_names: Vec<String>,
	values: Vec<String>,
	owner: Vec<&'static str>,
}

impl ListSetInfo {
	fn from_name_and_last_update(name: &str, updated: String) -> Self {
		ListSetInfo {
			name: name.to_owned(),
			last_updated: updated,
			..ListSetInfo::default()
		}
	}
}

#[derive(Template)]
#[template(path = "list_data.hbs")]
struct ListPage {
	datasets: Vec<ListSetInfo>,
}

pub async fn list_data(id: Identity, state: Data<DataRouterState>) -> impl Responder {
	let session_id = id
		.identity()
		.unwrap()
		.parse::<data_store::DatasetId>()
		.unwrap();
	let sessions = state.sessions.read().unwrap();
	let session = sessions.get(&session_id).unwrap();

	let mut infos = Vec::new();
	let datasets = &mut state.data.write().unwrap().sets;
	for (dataset_id, authorized_fields) in session
		.lock()
		.unwrap()
		.db_entry
		.timeseries_with_access
		.iter()
	{
		let set = datasets.get_mut(&dataset_id).unwrap();

		let time_since;
		let line = if let Ok((time, line)) = set.timeseries.decode_last_line() {
			time_since = format_to_duration(time);
			Some(line)
		} else {
			time_since = String::from(String::from("-"));
			None
		};

		let fields = &set.metadata.fields;
		let mut info = ListSetInfo::from_name_and_last_update(&set.metadata.name, time_since);

		for field in authorized_fields {
			let id = match field {
				Authorisation::Owner(id) => {
					info.owner.push("yes");
					id
				}
				Authorisation::Reader(id) => {
					info.owner.push("no");
					id
				}
			};
			if let Some(ref line) = line {
				info.values
					.push(fields[*id as usize].decode::<f32>(&line).to_string());
			} else {
				info.values.push(String::from("-"));
			}
			info.field_names
				.push(set.metadata.fields[*id as usize].name.clone());
		}
		infos.push(info);
	}

	let page = ListPage { datasets: infos };
	HttpResponse::Ok().body(page.call().unwrap())
}

struct PlotInfo {
	set_id: usize,
	field_id: usize,
	name: String,
}

struct PlotSetsInfo {
	dataset_name: String,
	infos: Vec<PlotInfo>,
}

#[derive(Template)]
#[template(path = "plot.hbs")]
struct PlotPage {
	datasets: Vec<PlotSetsInfo>,
}

pub async fn plot_data(id: Identity, state: Data<DataRouterState>) -> impl Responder {
	let session_id = id
		.identity()
		.unwrap()
		.parse::<data_store::DatasetId>()
		.unwrap();
	let sessions = state.sessions.read().unwrap();
	let session = sessions.get(&session_id).unwrap();

	let mut all_info = Vec::new();
	let data = state.data.read().unwrap();
	for (dataset_id, authorized_fields) in session
		.lock()
		.unwrap()
		.db_entry
		.timeseries_with_access
		.iter()
	{
		let mut infos = Vec::new();
		let metadata = &data
			.sets
			.get(&dataset_id)
			.expect("user has access to a database that does no longer exist")
			.metadata;
		for field_id in authorized_fields {
			let id = *field_id.as_ref() as usize;
			infos.push(PlotInfo {
				set_id: *dataset_id as usize,
				field_id: id,
				name: metadata.fields[id].name.clone(),
			});
		}

		all_info.push(PlotSetsInfo {
			dataset_name: metadata.name.clone(),
			infos,
		});
	}

	let page = PlotPage { datasets: all_info };
	HttpResponse::Ok().body(page.call().unwrap())
}
