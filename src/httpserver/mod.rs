extern crate actix;
extern crate actix_net;
extern crate actix_web;
extern crate actix_web_httpauth;

extern crate bytes;
extern crate futures;

extern crate env_logger;
extern crate rustls;
extern crate rand;
extern crate chrono;

use std::path::PathBuf;

use self::actix::Addr;
use self::actix::*;

use self::actix_web::middleware::identity::RequestIdentity;
use self::actix_web::middleware::identity::{CookieIdentityPolicy, IdentityService};
use self::actix_web::Error as wError;
use self::actix_web::Result as wResult;
use self::actix_web::{
	fs::NamedFile, http, http::Method, http::StatusCode, middleware, server, ws, App,
	AsyncResponder, Form, FutureResponse, HttpMessage, HttpRequest, HttpResponse, Responder,
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
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use self::chrono::{Utc};

pub mod timeseries_interface;
pub mod secure_database;

mod websocket_data_router;
mod websocket_client_handler;

use self::secure_database::{PasswordDatabase};
use timeseries_interface::{Authorisation,DatasetId};

pub struct Session {//TODO deprecate 
    userinfo: secure_database::UserInfo,
    //add more temporary user specific data as needed
}

pub struct WebServerData {
	passw_db: Arc<RwLock<PasswordDatabase>>,
	websocket_addr: Addr<websocket_data_router::DataServer>,
	data: Arc<RwLock<timeseries_interface::Data>>,
	sessions: Arc<RwLock<HashMap<u16,Session>>> ,
	free_session_ids: Arc<AtomicUsize>,
}

#[derive(Deserialize)]
struct Logindata {
	u: String,
	p: String,
}

type ServerHandle = self::actix::Addr<actix_net::server::Server>;
type DataHandle = self::actix::Addr<websocket_data_router::DataServer>;

fn serve_file(req: &HttpRequest<WebServerData>) -> wResult<NamedFile> {
	let file_name: String = req.match_info().query("tail")?;

	let mut path: PathBuf = PathBuf::from("web/");
	path.push(file_name);
	Ok(NamedFile::open(path)?)
}

fn index(req: &HttpRequest<WebServerData>) -> String {
	format!("Hello {}", req.identity().unwrap_or("Anonymous".to_owned()))
}

fn list_data(req: &HttpRequest<WebServerData>) -> HttpResponse {
	let mut accessible_fields = String::from("<html><body><table>");
	
	let session_id = req.identity().unwrap().parse::<timeseries_interface::DatasetId>().unwrap();
	let sessions = req.state().sessions.read().unwrap();
	let session = sessions.get(&session_id).unwrap();

	let data = req.state().data.read().unwrap();
	for (dataset_id, authorized_fields) in &session.userinfo.timeseries_with_access {
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

fn logout(req: &HttpRequest<WebServerData>) -> HttpResponse {
	req.forget();
	HttpResponse::Found().finish()
}

pub struct CheckLogin;
impl middleware::Middleware<WebServerData> for CheckLogin {
	// We only need to hook into the `start` for this middleware.
	fn start(&self, req: &HttpRequest<WebServerData>) -> wResult<middleware::Started> {
		if let Some(id) = req.identity() {
            //check if valid session
            if req.state().sessions.read().unwrap().contains_key(&id.parse().unwrap()) {
				return Ok(middleware::Started::Done);
			}
		}
		if req.path() == r"/newdata" { 
			//newdata is authenticated through other means
			return Ok(middleware::Started::Done);
		}
		// Don't forward to /login if we are already on /login
		if req.path().starts_with("/login") {
			return Ok(middleware::Started::Done);
		}

		let path = req.path();
		Ok(middleware::Started::Response(
			HttpResponse::Found()
				.header(http::header::LOCATION, "/login".to_owned() + path)
				.finish(),
		))
	}
}

fn login_page(_req: &HttpRequest<WebServerData>) -> HttpResponse {
	let page = include_str!("static_webpages/login.html");
	HttpResponse::Ok().header(http::header::CONTENT_TYPE, "text/html; charset=utf-8").body(page)
}

/// State and POST Params
fn login_get_and_check(
    (req, params): (HttpRequest<WebServerData>, Form<Logindata>),
) -> wResult<HttpResponse> {
	
	println!("checking login");
    //if login valid (check passwdb) load userinfo
    let state = req.state();
    let mut passw_db = state.passw_db.write().unwrap();
    
    if passw_db.verify_password(params.u.as_str().as_bytes(), params.p.as_str().as_bytes()).is_err(){
		println!("incorrect password");
		return Ok(HttpResponse::build(http::StatusCode::UNAUTHORIZED)
        .content_type("text/plain")
        .body("incorrect password or username"));
	} else { println!("user logged in");}
	
	//copy userinfo into new session
	let mut userinfo = passw_db.get_userdata(&params.u);
	userinfo.last_login = Utc::now();
	passw_db.set_userdata(params.u.as_str().as_bytes(), userinfo.clone());
	
    let session = Session {
		userinfo: userinfo,
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

fn newdata(req: &HttpRequest<WebServerData>) -> FutureResponse<HttpResponse> {
	
	let now = Utc::now();
	let data = req.state().data.clone();
	let websocket_addr = req.state().websocket_addr.clone(); //FIXME CLONE SHOULD NOT BE NEEDED
	req.body()
		.from_err()
		.and_then(move |bytes: Bytes| {
			let res = data.write().unwrap().store_new_data(bytes, now);
			match res {
				Ok((set_id, data_string)) => {
					websocket_addr.do_send(websocket_data_router::NewData {from: set_id, data: data_string.to_vec()}); 
					Ok(HttpResponse::Ok().status(StatusCode::OK).finish()) },
				Err(_) => Ok(HttpResponse::Ok().status(StatusCode::FORBIDDEN).finish()),
			}
		}).responder()
}

fn goodby(_req: &HttpRequest<WebServerData>) -> impl Responder {
	"Goodby!"
}

/// do websocket handshake and start `MyWebSocket` actor
fn ws_index(req: &HttpRequest<WebServerData>) -> Result<HttpResponse, wError> {
	println!("websocket connected");
	let session_id = req.identity().unwrap().parse::<u16>().unwrap();
	ws::start(req, websocket_client_handler::WsSession { session_id: session_id })
}

pub fn start(signed_cert: &Path, private_key: &Path, 
     data: Arc<RwLock<timeseries_interface::Data>>, 
     passw_db: Arc<RwLock<PasswordDatabase>>,
     sessions: Arc<RwLock<HashMap<u16,Session>>>) -> (DataHandle, ServerHandle) {
	// load ssl keys

	//if ::std::env::var("RUST_LOG").is_err() {
		//::std::env::set_var("RUST_LOG", "actix_web=trace");
	//}
	//env_logger::init();

	let mut config = ServerConfig::new(NoClientAuth::new());
	let cert_file = &mut BufReader::new(File::open(signed_cert).unwrap());
	let key_file = &mut BufReader::new(File::open(private_key).unwrap());
	let cert_chain = certs(cert_file).unwrap();
	let mut key = pkcs8_private_keys(key_file).unwrap();
	config
		.set_single_cert(cert_chain, key.pop().unwrap())
		.unwrap();

	let (tx, rx) = mpsc::channel();

    let free_session_ids = Arc::new(AtomicUsize::new(0));

	let mut cookie_private_key = [0u8; 32];
	let mut rng = rand::StdRng::from_entropy();
	rng.fill(&mut cookie_private_key[..]);

	thread::spawn(move || {
		// Start data server actor in separate thread
		let sys = actix::System::new("http-server");
		let data_server = Arbiter::start(|_| websocket_data_router::DataServer::default());
		let data_server_clone = data_server.clone();

		let web_server = server::new(move || {
			 // Websocket sessions state
			let state = WebServerData {
                passw_db: passw_db.clone(),
                websocket_addr: data_server_clone.clone(),
                data: data.clone(), 
                sessions: sessions.clone(),
                free_session_ids: free_session_ids.clone(),
            };
            
			App::with_state(state)
            .middleware(IdentityService::new(
                CookieIdentityPolicy::new(&cookie_private_key[..])
                    .domain("deviousd.duckdns.org")
                    .name("auth-cookie")
                    .path("/")
                    .secure(true),
            ))
			.middleware(CheckLogin)
                // websocket route
                // note some browsers need already existing http connection to 
                // this server for the upgrade to wss to work
                .resource("/ws/", |r| r.method(http::Method::GET).f(ws_index))
                .resource("/goodby.html", |r| r.f(goodby)) 
                .resource("/logout", |r| r.f(logout))
                .resource("/index", |r| r.f(index))
                .resource("/", |r| r.f(index))
                .resource(r"/newdata", |r| r.method(Method::POST).f(newdata))
                .resource(r"/list_data.html", |r| r.method(Method::GET).f(list_data))
                .resource(r"/login/{tail:.*}", |r| {
                        r.method(http::Method::POST).with(login_get_and_check);
                        r.method(http::Method::GET).f(login_page);
            })
            .resource(r"/{tail:.*}", |r| r.f(serve_file)) 
        })
        .bind_rustls("0.0.0.0:8080", config).unwrap()
        //.bind("0.0.0.0:8080").unwrap() //without tcp use with debugging (note: https -> http, wss -> ws)
        .shutdown_timeout(60)    // <- Set shutdown timeout to 60 seconds
        .start();

		let _ = tx.send((data_server, web_server));
		let _ = sys.run();
	});

	let (data_handle, web_handle) = rx.recv().unwrap();
	(data_handle, web_handle)
}

pub fn stop(handle: ServerHandle) {
	let _ = handle
		.send(server::StopServer { graceful: true })
		.timeout(Duration::from_secs(5)); // <- Send `StopServer` message to server.
}

pub fn signal_newdata(handle: DataHandle, set_id: timeseries_interface::DatasetId) {
	handle.do_send(websocket_data_router::NewData {
		from: set_id,
		data: vec!(5,10,3,4),
	});
	println!("send signal there is new data");
	//.timeout(Duration::from_secs(5));
}
