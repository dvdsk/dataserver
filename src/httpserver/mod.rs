extern crate bytes;
extern crate futures;

extern crate rustls;
extern crate rand;
extern crate chrono;

use std::path::PathBuf;

use actix::Addr;

use actix_identity::{CookieIdentityPolicy, IdentityService};
use actix_files::NamedFile;
use actix_web_actors::ws;
use actix_web::Error as wError;
use actix_web::Result as wResult;
use actix_web::{
	http, http::StatusCode, middleware, HttpServer,
	web, HttpMessage, HttpRequest, HttpResponse,
};

use self::bytes::Bytes;
use self::futures::future::Future;

use self::rustls::internal::pemfile::{certs, pkcs8_private_keys};
use self::rustls::{NoClientAuth, ServerConfig};
use self::rand::FromEntropy;
use self::rand::Rng;

use std::fs::File;
use std::io::BufReader;

use std::sync::{Arc, RwLock, atomic::{AtomicUsize,Ordering}};


use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use self::chrono::{DateTime, Utc};

pub mod timeseries_interface;
pub mod secure_database;

pub mod websocket_data_router;
pub mod websocket_client_handler;

use self::secure_database::{PasswordDatabase};
use crate::httpserver::timeseries_interface::{Authorisation};

pub struct Session {//TODO deprecate 
  timeseries_with_access: Arc<RwLock<HashMap<timeseries_interface::DatasetId, Vec<timeseries_interface::Authorisation>>>>,
	username: String,
	last_login: DateTime<Utc>,
  //add more temporary user specific data as needed
}

/// standardised interface that the libs handelers use to get the application state they need
pub trait InnerState {
	fn inner_state(&self) -> &DataServerState;
}

pub struct DataServerState {
	pub passw_db: Arc<RwLock<PasswordDatabase>>,
	pub websocket_addr: Addr<websocket_data_router::DataServer>,
	pub data: Arc<RwLock<timeseries_interface::Data>>,
	pub sessions: Arc<RwLock<HashMap<u16,Session>>> ,
	pub free_session_ids: Arc<AtomicUsize>,
	pub free_ws_session_ids: Arc<AtomicUsize>,
}

//allows to use
impl InnerState for DataServerState {
	fn inner_state(&self) -> &Self {
		self
	}
}

pub fn make_random_cookie_key() -> [u8; 32] {
	let mut cookie_private_key = [0u8; 32];
	let mut rng = rand::StdRng::from_entropy();
	rng.fill(&mut cookie_private_key[..]);
	cookie_private_key
}

pub fn make_tls_config<P: AsRef<Path>>(signed_cert_path: P, private_key_path: P) -> self::rustls::ServerConfig{
	let mut tls_config = ServerConfig::new(NoClientAuth::new());
	let cert_file = &mut BufReader::new(File::open(signed_cert_path).unwrap());
	let key_file = &mut BufReader::new(File::open(private_key_path).unwrap());
	let cert_chain = certs(cert_file).unwrap();
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
pub type DataRouterHandle = Addr<websocket_data_router::DataServer>;

// pub fn serve_file<T: InnerState>(req: &HttpRequest<T>) -> wResult<NamedFile> {
// 	let file_name: String = req.match_info().query("tail")?;

// 	let mut path: PathBuf = PathBuf::from("web/");
// 	path.push(file_name);
// 	trace!("returning file: {:?}", &path);
// 	Ok(NamedFile::open(path)?)
// }

pub fn index(req: &HttpRequest) -> String {
	format!("Hello {}", req.identity().unwrap_or("Anonymous".to_owned()))
}

pub fn list_data<T: InnerState>(state: web::Data<T>, req: &HttpRequest) -> HttpResponse {
	let mut accessible_fields = String::from("<html><body><table>");
	
	let session_id = req.identity().unwrap().parse::<timeseries_interface::DatasetId>().unwrap();
	let sessions = state.inner_state().sessions.read().unwrap();
	let session = sessions.get(&session_id).unwrap();

	let data = state.inner_state().data.read().unwrap();
	for (dataset_id, authorized_fields) in session.timeseries_with_access.read().unwrap().iter() {
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

pub fn plot_data<T: InnerState>(state: web::Data<T>, req: &HttpRequest) -> HttpResponse {
	let session_id = req.identity().unwrap().parse::<timeseries_interface::DatasetId>().unwrap();
	let sessions = state.inner_state().sessions.read().unwrap();
	let session = sessions.get(&session_id).unwrap();

	let before_form =include_str!("static_webpages/plot_A.html");
	let after_form = include_str!("static_webpages/plot_B.html");

	let mut page = String::from(before_form);
	let data = req.state().inner_state().data.read().unwrap();
	for (dataset_id, authorized_fields) in session.timeseries_with_access.read().unwrap().iter() {
		let metadata = &data.sets.get(&dataset_id).expect("user has access to a database that does no longer exist").metadata;
		for field_id in authorized_fields{
			let id = *field_id.as_ref() as usize;
			page.push_str(&format!("<input type=\"checkbox\" value={},{} > {}<br>\n", dataset_id, id, metadata.fields[id].name));
		}
	}
	page.push_str(after_form);
	HttpResponse::Ok().header(http::header::CONTENT_TYPE, "text/html; charset=utf-8").body(page)
}

fn plot_data_debug<T: InnerState>(state: web::Data<T>, req: &HttpRequest<T>) -> HttpResponse {
	let session_id = req.identity().unwrap().parse::<timeseries_interface::DatasetId>().unwrap();
	let sessions = state.inner_state().sessions.read().unwrap();
	let session = sessions.get(&session_id).unwrap();

	let before_form =include_str!("static_webpages/plot_A_debug.html");
	let after_form = include_str!("static_webpages/plot_B.html");

	let mut page = String::from(before_form);
	let data = req.state().inner_state().data.read().unwrap();
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

pub fn logout<T: InnerState>(req: &HttpRequest<T>) -> HttpResponse {
	req.forget();
	HttpResponse::Found().finish()
}

pub fn login_page(_req: &HttpRequest) -> HttpResponse {
	let page = include_str!("static_webpages/login.html");
	HttpResponse::Ok().header(http::header::CONTENT_TYPE, "text/html; charset=utf-8").body(page)
}

/// State and POST Params
pub fn login_get_and_check<T: InnerState>(
		state: web::State<T>,
		params: web::Form<Logindata>,
		req: HttpRequest) -> wResult<HttpResponse> {
	
	trace!("checking login");
    //if login valid (check passwdb) load userinfo
    let state = state.inner_state();
    let mut passw_db = state.passw_db.write().unwrap();
    
    if passw_db.verify_password(params.u.as_str().as_bytes(), params.p.as_str().as_bytes()).is_err(){
		warn!("incorrect password");
		return Ok(HttpResponse::build(http::StatusCode::UNAUTHORIZED)
        .content_type("text/plain")
        .body("incorrect password or username"));
	} else { info!("user logged in");}
	
	//copy userinfo into new session
	let userinfo = passw_db.get_userdata(&params.u);
	userinfo.last_login = Utc::now();
	//passw_db.set_userdata(params.u.as_str().as_bytes(), userinfo.clone());
	
    let session = Session {
		timeseries_with_access: Arc::new(RwLock::new(userinfo.timeseries_with_access.clone())),
		username: userinfo.username.clone(),
		last_login: userinfo.last_login.clone(),
	};
	//find free session_numb, set new session number and store new session
	let session_id = state.free_session_ids.fetch_add(1, Ordering::Acquire);
	let mut sessions = state.sessions.write().unwrap();
	sessions.insert(session_id as u16,session);
	
    //sign and send session id cookie to user 
    req.remember(session_id.to_string());
    
    Ok(HttpResponse::Found()
	   .header(http::header::LOCATION, req.path()["/login".len()..].to_owned())
	   .finish())
}

pub fn newdata<T: InnerState+'static>(
	state: web::State<T>, 
	req: &HttpRequest)
	 -> FutureResponse<HttpResponse> {
	
	trace!("newdata");
	let now = Utc::now();
	let data = state.inner_state().data.clone();//clones pointer
	let websocket_addr = state.inner_state().websocket_addr.clone(); //FIXME CLONE SHOULD NOT BE NEEDED
	trace!("got addr");
	req.body()
		.from_err()
		.and_then(move |bytes: Bytes| {
			trace!("trying to get data");
			let mut data = data.write().unwrap();
			trace!("got data lock");
			match data.store_new_data(bytes, now) {
				Ok((set_id, data_string)) => {
					trace!("stored new data");
					websocket_addr.do_send(websocket_data_router::NewData {
						from_id: set_id,
						line: data_string,
						timestamp: now.timestamp()
					});
					trace!("done websocket send");
					Ok(HttpResponse::Ok().status(StatusCode::OK).finish()) },
				Err(_) => Ok(HttpResponse::Ok().status(StatusCode::FORBIDDEN).finish()),
			}
		}).responder()
}

/// do websocket handshake and start `MyWebSocket` actor
pub fn ws_index<T: InnerState+'static>(
	state: web::State<T>, 
	req: &HttpRequest) -> wResult<HttpResponse> {

	trace!("websocket connected");
	let session_id = req.identity().unwrap().parse::<u16>().unwrap();
	let sessions = state.inner_state().sessions.read().unwrap();
	let session = sessions.get(&session_id).unwrap();
	
	let timeseries_with_access = session.timeseries_with_access.clone();
	let ws_session_id = req.state().inner_state().free_session_ids.fetch_add(1, Ordering::Acquire);
	
	ws::start(req, websocket_client_handler::WsSession {
		http_session_id: session_id,
		ws_session_id: ws_session_id  as u16,
		selected_data: HashMap::new(),
		timerange: websocket_client_handler::TimesRange::default(),
		compression_enabled: true,
		timeseries_with_access: timeseries_with_access,
		file_io_thread: None,
		phantom: std::marker::PhantomData,
	})
}

pub fn stop(handle: ServerHandle) {
	let _ = handle
		.send(server::StopServer { graceful: true })
		.timeout(Duration::from_secs(5)); // <- Send `StopServer` message to server.
}

pub fn signal_newdata(handle: &DataRouterHandle, from_id: timeseries_interface::DatasetId, line: Vec<u8>, timestamp: i64) {
	handle.do_send(websocket_data_router::NewData {
		from_id,
		line,
		timestamp,
	});
	//.timeout(Duration::from_secs(5));
}
