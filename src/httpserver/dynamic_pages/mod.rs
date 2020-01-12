use actix_web::{HttpResponse, Responder, http};
use actix_web::web::{Data};
use actix_identity::Identity;

extern crate yarte;
use yarte::Template;

use crate::data_store;
use data_store::{Authorisation, data_router::DataRouterState};

// pub fn settings(id: Identity, state: Data<DataRouterState>) -> HttpResponse {
// 	let mut accessible_fields = String::from("<html><body><table>");
	
// 	let session_id = id.identity().unwrap().parse::<data_store::DatasetId>().unwrap();
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

pub async fn settings_page(_id: Identity, _state: Data<DataRouterState>) -> impl Responder {
    SettingsPage  {
        telegram_id: "test",
    }
}

pub fn list_data(id: Identity, state: Data<DataRouterState>) -> HttpResponse {
	let mut accessible_fields = String::from("<html><body><table>");
	
	let session_id = id.identity().unwrap().parse::<data_store::DatasetId>().unwrap();
	let sessions = state.sessions.read().unwrap();
	let session = sessions.get(&session_id).unwrap();

	let data = state.data.read().unwrap();
	for (dataset_id, authorized_fields) in session.lock().unwrap().db_entry.timeseries_with_access.iter() {
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