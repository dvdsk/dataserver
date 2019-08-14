use serde::{ Deserialize};
use log::{warn, info, trace};
use chrono;

use actix::Addr;
use actix_identity::{Identity};
use actix_web_actors::ws;
use actix_web::Result as wResult;
use actix_web::{
	http, http::StatusCode,
	HttpRequest, HttpResponse,
};
use actix_web::web::{Data, Form, Bytes, Payload};

use rustls::internal::pemfile::{certs, pkcs8_private_keys};
use rustls::{NoClientAuth, ServerConfig};
use rand::FromEntropy;
use rand::Rng;

use std::fs::File;
use std::io::BufReader;

use std::sync::{Arc, RwLock, atomic::{AtomicUsize,Ordering}, Mutex};

use std::collections::HashMap;
use std::path::Path;
use chrono::{DateTime, Utc};

pub mod timeseries_interface;
pub mod secure_database;
pub mod login_redirect;

pub mod data_router;
pub mod error_router;
pub mod data_router_ws_client; //TODO remove pub
mod error_router_ws_client;

use secure_database::{PasswordDatabase, UserDatabase};
use crate::httpserver::timeseries_interface::{Authorisation};

pub struct Session {//TODO deprecate 
	timeseries_with_access: HashMap<timeseries_interface::DatasetId, Vec<timeseries_interface::Authorisation>>,
	username: String,
	last_login: DateTime<Utc>,
  //add more temporary user specific data as needed
}

/// standardised interface that the libs handelers use to get the application state they need
pub trait InnerState {
	fn inner_state(&self) -> &DataRouterState;
	//fn into_inner(self) -> DataRouterState;
}

pub struct DataRouterState {
	pub passw_db: PasswordDatabase,
	pub user_db: UserDatabase,

	pub data_router_addr: Addr<data_router::DataRouter>,
	pub error_router_addr: Addr<error_router::ErrorRouter>,

	pub data: Arc<RwLock<timeseries_interface::Data>>,

	pub sessions: Arc<RwLock<HashMap<u16, Arc<Mutex<Session>> >>> ,
	pub free_session_ids: Arc<AtomicUsize>,
	pub free_ws_session_ids: Arc<AtomicUsize>,
}

//allows to use
impl InnerState for DataRouterState {
	fn inner_state(&self) -> &Self {
		&self
	}
	//fn into_inner(self) -> Self {
	//	self
	//}
}

pub fn make_random_cookie_key() -> [u8; 32] {
	let mut cookie_private_key = [0u8; 32];
	let mut rng = rand::StdRng::from_entropy();
	rng.fill(&mut cookie_private_key[..]);
	cookie_private_key
}

pub fn make_tls_config<P: AsRef<Path>>(cert_path: P, key_path: P, 
    intermediate_cert_path: P) 
-> rustls::ServerConfig{

	let mut tls_config = ServerConfig::new(NoClientAuth::new());
	let cert_file = &mut BufReader::new(File::open(cert_path).unwrap());
	let intermediate_file = &mut BufReader::new(File::open(intermediate_cert_path).unwrap());
	let key_file = &mut BufReader::new(File::open(key_path).unwrap());
	
	let mut cert_chain = certs(cert_file).unwrap();
	cert_chain.push(certs(intermediate_file).unwrap().pop().unwrap());

	let mut key = pkcs8_private_keys(key_file).unwrap();

	tls_config
		.set_single_cert(cert_chain, key.pop().unwrap())
		.unwrap();
	tls_config
}

#[derive(Deserialize)]
pub struct Logindata {
	u: String,
	p: String,
}

pub type ServerHandle = Addr<actix_net::server::Server>;
pub type DataRouterHandle = Addr<data_router::DataRouter>;
pub type ErrorRouterHandle = Addr<error_router::ErrorRouter>;

// pub fn serve_file<T: InnerState>(req: &HttpRequest) -> wResult<NamedFile> {
// 	let file_name: String = req.match_info().query("tail")?;

// 	let mut path: PathBuf = PathBuf::from("web/");
// 	path.push(file_name);
// 	trace!("returning file: {:?}", &path);
// 	Ok(NamedFile::open(path)?)
// }

pub fn index(id: Identity) -> String {
	format!("Hello {}", id.identity().unwrap_or_else(||"Anonymous".to_owned()))
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

pub fn plot_data<T: InnerState>(id: Identity, state: Data<T>) -> HttpResponse {
	let session_id = id.identity().unwrap().parse::<timeseries_interface::DatasetId>().unwrap();
	let sessions = state.inner_state().sessions.read().unwrap();
	let session = sessions.get(&session_id).unwrap();

	let before_form =include_str!("static_webpages/plot_A.html");
	let after_form = include_str!("static_webpages/plot_B.html");

	let mut page = String::from(before_form);
	let data = state.inner_state().data.read().unwrap();
	for (dataset_id, authorized_fields) in session.lock().unwrap().timeseries_with_access.iter() {
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
fn plot_data_debug<T: InnerState>(id: Identity, state: Data<T>, req: &HttpRequest) -> HttpResponse {
	let session_id = id.identity().unwrap().parse::<timeseries_interface::DatasetId>().unwrap();
	let sessions = state.inner_state().sessions.read().unwrap();
	let session = sessions.get(&session_id).unwrap();

	let before_form =include_str!("static_webpages/plot_A_debug.html");
	let after_form = include_str!("static_webpages/plot_B.html");

	let mut page = String::from(before_form);
	let data = state.inner_state().data.read().unwrap();
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

pub fn logout<T: InnerState>(id: Identity) -> HttpResponse {
	id.forget();
	HttpResponse::Found().finish()
}

pub fn login_page() -> HttpResponse {
	let page = include_str!("static_webpages/login.html");
	HttpResponse::Ok().header(http::header::CONTENT_TYPE, "text/html; charset=utf-8").body(page)
}

/// State and POST Params
pub fn login_get_and_check<T: InnerState>(
		id: Identity,
		state: Data<T>,
		req: HttpRequest,
		params: Form<Logindata>) -> wResult<HttpResponse> {
	
	trace!("checking login");

	//time function duration
	let now = std::time::Instant::now();

    //if login valid (check passwdb) load userinfo
    let state = &mut state.inner_state();

    if state.passw_db.verify_password(params.u.as_str().as_bytes(), params.p.as_str().as_bytes()).is_err(){
		warn!("incorrect password");
		return Ok(HttpResponse::build(http::StatusCode::UNAUTHORIZED)
        .content_type("text/plain")
        .body("incorrect password or username"));
	} else { info!("user logged in");}
	
	//copy userinfo into new session
	let userinfo = state.user_db.get_userdata(&params.u).unwrap();
	//userinfo.last_login = Utc::now();
	//passw_db.set_userdata(params.u.as_str().as_bytes(), userinfo.clone());
	
    let session = Session {
		timeseries_with_access: userinfo.timeseries_with_access.clone(),
		username: userinfo.username.clone(),
		last_login: chrono::Utc::now(), //TODO write back to database
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

pub fn new_data_post<T: InnerState+'static>(state: Data<T>, body: Bytes)
	 -> HttpResponse {
	
	let now = Utc::now();
	let data = state.inner_state().data.clone();//clones pointer
	let data_router_addr = state.inner_state().data_router_addr.clone(); //FIXME CLONE SHOULD NOT BE NEEDED

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
pub fn data_router_ws_index<T: InnerState+'static>(
	id: Identity,
	state: Data<T>, 
	req: HttpRequest,
	stream: Payload,
) -> wResult<HttpResponse> {

	info!("websocket connected");
	let session_id = id.identity().unwrap().parse::<u16>().unwrap();
	let sessions = state.inner_state().sessions.read().unwrap();
	let session = sessions.get(&session_id).unwrap();
	
	let session_clone = session.clone();//TODO security do we want clone here?
	let ws_session_id = state.inner_state().free_session_ids.fetch_add(1, Ordering::Acquire);
	
	let ws_session: data_router_ws_client::WsSession = data_router_ws_client::WsSession {
		http_session_id: session_id,
		ws_session_id: ws_session_id  as u16,
		selected_data: HashMap::new(),
		timerange: data_router_ws_client::TimesRange::default(),
		compression_enabled: true,
		session: session_clone,
		file_io_thread: None,

		data_router_addr: state.inner_state().data_router_addr.clone(),
		data: state.inner_state().data.clone(),
	};

	ws::start(
		ws_session,
		&req,
		stream,
	)
}

/// do websocket handshake and start `MyWebSocket` actor
pub fn error_router_ws_index<T: InnerState+'static>(
	id: Identity,
	state: Data<T>, 
	req: HttpRequest,
	stream: Payload,
) -> wResult<HttpResponse> {

	trace!("websocket connected");
	let session_id = id.identity().unwrap().parse::<u16>().unwrap();
	let sessions = state.inner_state().sessions.read().unwrap();
	let session = sessions.get(&session_id).unwrap();
	
	let session_clone = session.clone();//TODO security do we want clone here?
	let ws_session_id = state.inner_state().free_session_ids.fetch_add(1, Ordering::Acquire);
	
	let ws_session: error_router_ws_client::WsSession = error_router_ws_client::WsSession {
		http_session_id: session_id,
		ws_session_id: ws_session_id  as u16,
		session: session_clone,
		router_addr: state.inner_state().error_router_addr.clone(),
	};

	ws::start(
		ws_session,
		&req,
		stream,
	)
}

//TODO customise
pub fn new_error_post<T: InnerState+'static>(state: Data<T>, body: Bytes)
	 -> HttpResponse {
	
	let now = Utc::now();
	let data = state.inner_state().data.clone();//clones pointer
	let error_router_addr = state.inner_state().error_router_addr.clone(); //FIXME CLONE SHOULD NOT BE NEEDED

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

/*
pub fn stop(handle: ServerHandle) {
	let _ = handle
		.send(server::StopServer { graceful: true })
		.timeout(Duration::from_secs(5)); // <- Send `StopServer` message to server.
}*/

pub fn signal_newdata(handle: &DataRouterHandle, from_id: timeseries_interface::DatasetId, line: Vec<u8>, timestamp: i64) {
	handle.do_send(data_router::NewData {
		from_id,
		line,
		timestamp,
	});
	//.timeout(Duration::from_secs(5));
}
