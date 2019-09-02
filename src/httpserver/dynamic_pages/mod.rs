use super::InnerState;
use actix_web::{HttpResponse, Responder, http};
use actix_web::web::{Data};
use actix_identity::Identity;

extern crate yarte;
use yarte::Template;

use super::timeseries_interface;
use timeseries_interface::{Authorisation};

// pub fn settings<T: InnerState>(id: Identity, state: Data<T>) -> HttpResponse {
// 	let mut accessible_fields = String::from("<html><body><table>");
	
// 	let session_id = id.identity().unwrap().parse::<timeseries_interface::DatasetId>().unwrap();
// 	let sessions = state.inner_state().sessions.read().unwrap();
// 	let session = sessions.get(&session_id).unwrap();

// 	let data = state.inner_state().data.read().unwrap();
// 	for (dataset_id, authorized_fields) in session.lock().unwrap().timeseries_with_access.iter() {
// 		let metadata = &data.sets.get(&dataset_id).unwrap().metadata;
// 		let mut dataset_fields = format!("<th>{}</th>", &metadata.name);
		
// 		for field in authorized_fields{
// 			match field{
// 				Authorisation::Owner(id) => dataset_fields.push_str(&format!("<td><p><i>{}</i></p></td>", metadata.fields[*id as usize].name)),
// 				Authorisation::Reader(id) => dataset_fields.push_str(&format!("<td>{}</td>",metadata.fields[*id as usize].name)),
// 			};
// 		}
// 		accessible_fields.push_str(&format!("<tr>{}</tr>",&dataset_fields));
// 	}
// 	accessible_fields.push_str("</table></body></html>");
// 	HttpResponse::Ok().header(http::header::CONTENT_TYPE, "text/html; charset=utf-8").body(accessible_fields)
// }


#[derive(Template)]
#[template(path = "settings.hbs")]
struct SettingsPage<'a> {
    telegram_id: &'a str,
}

pub fn settings_page<T: InnerState>(id: Identity, state: Data<T>) -> impl Responder {
    SettingsPage  {
        telegram_id: "test",
    }
}

pub fn list_data<T: InnerState>(id: Identity, state: Data<T>) -> HttpResponse {
	let mut accessible_fields = String::from("<html><body><table>");
	
	let session_id = id.identity().unwrap().parse::<timeseries_interface::DatasetId>().unwrap();
	let sessions = state.inner_state().sessions.read().unwrap();
	let session = sessions.get(&session_id).unwrap();

	let data = state.inner_state().data.read().unwrap();
	for (dataset_id, authorized_fields) in session.lock().unwrap().timeseries_with_access.iter() {
		let metadata = &data.sets.get(&dataset_id).unwrap().metadata;
		let mut dataset_fields = format!("<th>{}</th>", &metadata.name);
		
		for field in authorized_fields{
			match field{
				Authorisation::Owner(id) => dataset_fields.push_str(&format!("<td><p><i>{}</i></p></td>", metadata.fields[*id as usize].name)),
				Authorisation::Reader(id) => dataset_fields.push_str(&format!("<td>{}</td>",metadata.fields[*id as usize].name)),
			};
		}
		accessible_fields.push_str(&format!("<tr>{}</tr>",&dataset_fields));
	}
	accessible_fields.push_str("</table></body></html>");
	HttpResponse::Ok().header(http::header::CONTENT_TYPE, "text/html; charset=utf-8").body(accessible_fields)
}