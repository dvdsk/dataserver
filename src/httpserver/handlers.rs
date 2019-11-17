use serde::{ Deserialize};
use log::{warn, info, trace};
use chrono;

use actix_identity::{Identity};
use actix_web_actors::ws;
use actix_web::Result as wResult;
use actix_web::{
	http, http::StatusCode,
	HttpRequest, HttpResponse,
};
use actix_web::web::{Data, Form, Bytes, Payload};

use std::sync::{Arc, atomic::{Ordering}, Mutex};

use std::collections::HashMap;
use chrono::{Utc};

use crate::databases::{BotUserInfo};
use crate::data_store::{data_router, data_router::DataRouterState, error_router};
use crate::data_store;

use super::{Session, data_router_ws_client, error_router_ws_client};

pub fn index() -> HttpResponse {
	let index_page = std::include_str!("static_webpages/index.html");
	HttpResponse::Ok()
		.header(http::header::CONTENT_TYPE, "text/html; charset=utf-8")
		.body(index_page)
}

pub fn plot_data(id: Identity, state: Data<DataRouterState>) -> HttpResponse {
	let session_id = id.identity().unwrap().parse::<data_store::DatasetId>().unwrap();
	let sessions = state.sessions.read().unwrap();
	let session = sessions.get(&session_id).unwrap();

	let before_form =include_str!("static_webpages/plot_A.html");
	let after_form = include_str!("static_webpages/plot_B.html");

	let mut page = String::from(before_form);
	let data = state.data.read().unwrap();
	for (dataset_id, authorized_fields) in session.lock().unwrap().db_entry.timeseries_with_access.iter() {
		let metadata = &data.sets.get(&dataset_id).expect("user has access to a database that does no longer exist").metadata;
		for field_id in authorized_fields{
			let id = *field_id.as_ref() as usize;
			page.push_str(&format!("<input type=\"checkbox\" value={},{} > {}<br>\n", dataset_id, id, metadata.fields[id].name));
		}
	}
	page.push_str(after_form);
	HttpResponse::Ok().header(http::header::CONTENT_TYPE, "text/html; charset=utf-8").body(page)
}

/*
fn plot_data_debugD(id: Identity, state: Data<DataRouterState>, req: &HttpRequest) -> HttpResponse {
	let session_id = id.identity().unwrap().parse::<data_store::DatasetId>().unwrap();
	let sessions = state.sessions.read().unwrap();
	let session = sessions.get(&session_id).unwrap();

	let before_form =include_str!("static_webpages/plot_A_debug.html");
	let after_form = include_str!("static_webpages/plot_B.html");

	let mut page = String::from(before_form);
	let data = state.data.read().unwrap();
	for (dataset_id, authorized_fields) in session.timeseries_with_access.read().unwrap().iter() {
		let metadata = &data.sets.get(&dataset_id).unwrap().metadata;
		for field_id in authorized_fields{
			let id = *field_id.as_ref() as usize;
			page.push_str(&format!("<input type=\"checkbox\" value={},{} > {}<br>\n", dataset_id, id, metadata.fields[id].name));
		}
	}
	page.push_str(after_form);
	HttpResponse::Ok().header(http::header::CONTENT_TYPE, "text/html; charset=utf-8").body(page)
}
*/

pub fn logout(id: Identity) -> HttpResponse {
	id.forget();
	HttpResponse::Found().finish()
}

pub fn login_page() -> HttpResponse {
	let page = include_str!("static_webpages/login.html");
	HttpResponse::Ok().header(http::header::CONTENT_TYPE, "text/html; charset=utf-8").body(page)
}

#[derive(Deserialize)]
pub struct Logindata {
	u: String,
	p: String,
}

/// State and POST Params
pub fn login_get_and_check(
		id: Identity,
		state: Data<DataRouterState>,
		req: HttpRequest,
		params: Form<Logindata>) -> wResult<HttpResponse> {
	
	trace!("checking login");

	//time function duration
	let now = std::time::Instant::now();

    //if login valid (check passwdb) load userinfo
    let state = &mut state.get_ref();

    if state.passw_db.verify_password(params.u.as_str().as_bytes(), params.p.as_str().as_bytes()).is_err(){
		warn!("incorrect password");
		return Ok(HttpResponse::build(http::StatusCode::UNAUTHORIZED)
        .content_type("text/plain")
        .body("incorrect password or username"));
	} else { info!("user logged in");}
	
	//copy userinfo into new session
	let userinfo = state.web_user_db.get_userdata(&params.u).unwrap();
	//userinfo.last_login = Utc::now();
	//passw_db.set_userdata(params.u.as_str().as_bytes(), userinfo.clone());
	
	let session = Session {
		db_entry: userinfo,
	};

	//find free session_numb, set new session number and store new session
	let session_id = state.free_session_ids.fetch_add(1, Ordering::Acquire);
	let mut sessions = state.sessions.write().unwrap();
	sessions.insert(session_id as u16, Arc::new(Mutex::new(session)));
	
	//sign and send session id cookie to user 
	id.remember(session_id.to_string());
	info!("remembering session");
	
	let end = std::time::Instant::now();
	println!("{:?}", end-now);

	Ok(HttpResponse::Found()
	.header(http::header::LOCATION, req.path()["/login".len()..].to_owned())
	.finish())
}

#[derive(Deserialize)]
pub struct TelegramId {
	id: String,
}

pub fn set_telegram_id_post(
		id: Identity,
		state: Data<DataRouterState>,
		params: Form<TelegramId>) -> wResult<HttpResponse> {
	
	//needs reimplementation, look at implementation in menu
	
	Ok(HttpResponse::Ok().finish())
}

pub fn new_data_post(state: Data<DataRouterState>, body: Bytes)
	 -> HttpResponse {
	
	let now = Utc::now();
	let data = state.data.clone();//clones pointer
	let data_router_addr = state.data_router_addr.clone(); //FIXME CLONE SHOULD NOT BE NEEDED

	let mut data = data.write().unwrap();
	match data.store_new_data(body, now) {
		Ok((set_id, data_string)) => {
			trace!("stored new data");
			data_router_addr.do_send(data_router::NewData {
				from_id: set_id,
				line: data_string,
				timestamp: now.timestamp()
			});
			HttpResponse::Ok().status(StatusCode::OK).finish() },
		Err(_) => HttpResponse::Ok().status(StatusCode::FORBIDDEN).finish(),
	}
}

/// do websocket handshake and start `MyWebSocket` actor
pub fn data_router_ws_index(
	id: Identity,
	state: Data<DataRouterState>, 
	req: HttpRequest,
	stream: Payload,
) -> wResult<HttpResponse> {

	info!("websocket connected");
	let session_id = id.identity().unwrap().parse::<u16>().unwrap();
	let sessions = state.sessions.read().unwrap();
	let session = sessions.get(&session_id).unwrap();
	
	let session_clone = session.clone();//TODO security do we want clone here?
	let ws_session_id = state.free_session_ids.fetch_add(1, Ordering::Acquire);
	
	let ws_session: data_router_ws_client::WsSession = data_router_ws_client::WsSession {
		http_session_id: session_id,
		ws_session_id: ws_session_id  as u16,
		selected_data: HashMap::new(),
		timerange: data_router_ws_client::TimesRange::default(),
		compression_enabled: true,
		session: session_clone,
		file_io_thread: None,

		data_router_addr: state.data_router_addr.clone(),
		data: state.data.clone(),
	};

	ws::start(
		ws_session,
		&req,
		stream,
	)
}

/// do websocket handshake and start `MyWebSocket` actor
pub fn error_router_ws_index(
	id: Identity,
	state: Data<DataRouterState>, 
	req: HttpRequest,
	stream: Payload,
) -> wResult<HttpResponse> {

	trace!("websocket connected");
	let session_id = id.identity().unwrap().parse::<u16>().unwrap();
	let sessions = state.sessions.read().unwrap();
	let session = sessions.get(&session_id).unwrap();
	
	let session_clone = session.clone();//TODO security do we want clone here?
	let ws_session_id = state.free_session_ids.fetch_add(1, Ordering::Acquire);
	
	let ws_session: error_router_ws_client::WsSession = error_router_ws_client::WsSession {
		http_session_id: session_id,
		ws_session_id: ws_session_id  as u16,
		session: session_clone,
		router_addr: state.error_router_addr.clone(),
	};

	ws::start(
		ws_session,
		&req,
		stream,
	)
}

//TODO customise
pub fn new_error_post(state: Data<DataRouterState>, body: Bytes)
	 -> HttpResponse {
	let now = Utc::now();
	let data = state.data.clone();//clones pointer
	let error_router_addr = state.error_router_addr.clone(); //FIXME CLONE SHOULD NOT BE NEEDED

	let mut data = data.write().unwrap();
	match data.authenticate_error_packet(&body) {
		Ok(dataset_id) => {
			let error_code = body[10];
			let field_ids = body.into_iter().skip(11).collect();
			error_router_addr.do_send(error_router::NewError {
				dataset_id,
				field_ids,
				error_code,
				timestamp: now,
			});
			HttpResponse::Ok().status(StatusCode::OK).finish() 
		},
		Err(_) => HttpResponse::Ok().status(StatusCode::FORBIDDEN).finish(),
	}
}
