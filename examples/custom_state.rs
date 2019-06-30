extern crate dataserver;

extern crate actix_web;

use crate::actix_web::actix::Arbiter;
use crate::actix_web::{HttpServer,App,http::Method};
use crate::actix_identity::{CookieIdentityPolicy, IdentityService};

use std::sync::mpsc;
use std::sync::atomic::{AtomicUsize};
use std::thread;

use dataserver::{certificate_manager, httpserver};
use dataserver::{helper};
use dataserver::httpserver::{InnerState, secure_database::PasswordDatabase, timeseries_interface, ServerHandle, DataRouterHandle, DataServerState, CheckLogin};
use dataserver::httpserver::{ws_index, index, logout, newdata, plot_data, list_data, login_get_and_check, login_page, serve_file};

use std::sync::{Arc, RwLock, Mutex};
use std::io::stdin;
use std::collections::HashMap;

const FORCE_CERT_REGEN: bool =	false;

struct ExampleState {
	counter: Arc<Mutex<usize>>,
	dataserver_state: DataServerState,
}

/// simple handle
fn test_state(req: &actix_web::HttpRequest<ExampleState>) -> actix_web::HttpResponse {
    println!("{:?}", req);
    *(req.state().counter.lock().unwrap()) += 1;

    actix_web::HttpResponse::Ok().body(format!("Num of requests: {}", req.state().counter.lock().unwrap()))
}

impl InnerState for ExampleState{
	fn inner_state(&self) -> &DataServerState {
		&self.dataserver_state
	}
}

pub fn start(signed_cert: &str, private_key: &str,
     data: Arc<RwLock<timeseries_interface::Data>>, //
     passw_db: Arc<RwLock<PasswordDatabase>>,
     sessions: Arc<RwLock<HashMap<u16, dataserver::httpserver::Session>>>) -> (DataRouterHandle, ServerHandle) {

	let tls_config = httpserver::make_tls_config(signed_cert, private_key);
	let cookie_key = httpserver::make_random_cookie_key();

  let free_session_ids = Arc::new(AtomicUsize::new(0));
	let free_ws_session_ids = Arc::new(AtomicUsize::new(0));

	let (tx, rx) = mpsc::channel();
	thread::spawn(move || {
		// Start data server actor in separate thread
		let sys = actix::System::new("http-server");
		let data_server = Arbiter::start(|_| httpserver::websocket_data_router::DataServer::default());
		let data_server_clone = data_server.clone();

		let web_server = HttpServer::new(move || {
			// data the webservers functions have access to
			let state = ExampleState {
				counter: Arc::new(Mutex::new(0)),
				dataserver_state: DataServerState {
					passw_db: passw_db.clone(),
					websocket_addr: data_server_clone.clone(),
					data: data.clone(),
					sessions: sessions.clone(),
					free_session_ids: free_session_ids.clone(),
					free_ws_session_ids: free_ws_session_ids.clone(),
				},
		  };
			App::new()
				.data(state)
		    .wrap(IdentityService::new(
		      CookieIdentityPolicy::new(&cookie_key[..])
		      .domain("deviousd.duckdns.org")
		      .name("auth-cookie")
		      .path("/")
		      .secure(true),
		    ))
				.middleware(CheckLogin{
					public_roots: vec!(String::from("/commands")),
					..CheckLogin::default()
				})
				.service(
					web::scope("/") //TODO redo from examples (https://github.com/actix/examples/blob/master/basics/src/main.rs)
						.service(web::resource("commands/test_state").to(test_state))
						.service(web::resource("ws/").to(ws_index))
						.service(web::resource("logout").to(logout))
						.service(web::resource("").to(index))
						.service(web::resource("newdata").to(newdata))
						.service(web::resource("plot").to(plot_data))
						.service(web::resource("list_data").to(list_data))
						.service(web::resource(r"login/{path}").route(
							web::get()
								.method(http::Method::POST)
								.to(login_get_and_check)
						))
						.service(web::resource(r"login/{tail:.*}").route(
							web::get()
								.method(http::Method::GET)
								.to(login_page)
						))
						//for all other urls we try to resolve to static files in the "web" dir
						.service(fs::Files::new("", "."))
						//.service(web::resource(r"/{tail:.*}").route(serve_file))
    		)
    		.bind_rustls("0.0.0.0:8080", tls_config).unwrap()
    //.bind("0.0.0.0:8080").unwrap() //without tcp use with debugging (note: https -> http, wss -> ws)
    		.shutdown_timeout(5)    // shut down 5 seconds after getting the signal to shut down
    		.start(); // end of App::new()

		let _ = tx.send((data_server, web_server));
		let _ = sys.run();
	});

	let (data_handle, web_handle) = rx.recv().unwrap();
	(data_handle, web_handle)
}

fn main() {
	//https://www.deviousd.duckdns.org:8080/index.html
	//only do if certs need update
	if FORCE_CERT_REGEN {
		//generate_and_sign_keys
		if let Err(error) = certificate_manager::generate_and_sign_keys(
			"deviousd.duckdns.org",
			"keys/cert.key",
			"keys/cert.cert",
			"keys/user.key",
		) {
			println!("could not auto generate certificate, error: {:?}", error)
		}
	}

	helper::setup_logging(2).expect("could not set up debugging");

	let passw_db = Arc::new(RwLock::new(PasswordDatabase::load("").unwrap()));
	let data = Arc::new(RwLock::new(timeseries_interface::init("data").unwrap()));
	let sessions = Arc::new(RwLock::new(HashMap::new()));

	let (data_handle, web_handle) =
	start("keys/cert.key", "keys/cert.cert", data.clone(), passw_db.clone(), sessions.clone());
	println!("press: t to send test data, n: to add a new user, q to quit, a to add new dataset");
	loop {
		let mut input = String::new();
		stdin().read_line(&mut input).unwrap();
		match input.as_str() {
			"t\n" => helper::send_test_data_over_http(data.clone(), 8070),
			"d\n" => helper::signal_and_append_test_data(data.clone(), &data_handle), //works
			"n\n" => helper::add_user(& passw_db),
			"a\n" => helper::add_dataset(&passw_db, &data),
			"q\n" => break,
			_ => println!("unhandled"),
		};
	}
	println!("shutting down");
	httpserver::stop(web_handle);
}
