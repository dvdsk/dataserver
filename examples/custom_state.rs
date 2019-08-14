use actix_web::{HttpServer,App, web, HttpRequest};
use actix_identity::{CookieIdentityPolicy, IdentityService};
use actix_files as fs;

use actix::prelude::*;

use std::sync::mpsc;
use std::sync::atomic::{AtomicUsize};
use std::thread;

use dataserver::{certificate_manager, httpserver};
use dataserver::{helper};
use dataserver::httpserver::{InnerState, timeseries_interface, DataRouterHandle, ErrorRouterHandle, DataRouterState};
use dataserver::httpserver::{data_router_ws_index, error_router_ws_index, index, logout, plot_data, list_data, login_get_and_check, login_page};
use dataserver::httpserver::{new_data_post, new_error_post};

use dataserver::httpserver::secure_database::{PasswordDatabase, UserDatabase};
use dataserver::httpserver::login_redirect::CheckLogin;
use dataserver::telegram_bot::{handle_bot_message, set_webhook};

use std::sync::{Arc, RwLock, Mutex};
use std::io::stdin;
use std::collections::HashMap;

mod debug_middleware;
mod config;
const FORCE_CERT_REGEN: bool =	false;

struct ExampleState {
	counter: Arc<Mutex<usize>>,
	dataserver_state: DataRouterState,
}

/// simple handle
fn test_state(req: HttpRequest, state: actix_web::web::Data<ExampleState>) -> actix_web::HttpResponse {
    println!("{:?}", req);
    *(state.counter.lock().unwrap()) += 1;

    actix_web::HttpResponse::Ok().body(format!("Num of requests: {}", state.counter.lock().unwrap()))
}

impl InnerState for ExampleState{
	fn inner_state(&self) -> &DataRouterState {
		&self.dataserver_state
	}
}

pub fn start(signed_cert: &str, public_key: &str, intermediate_cert: &str,
     data: Arc<RwLock<timeseries_interface::Data>>, //
     passw_db: PasswordDatabase,
	 user_db: UserDatabase,
	 db: sled::Db,
     sessions: Arc<RwLock<HashMap<u16, Arc<Mutex<dataserver::httpserver::Session>>>>>)
	  -> (DataRouterHandle, ErrorRouterHandle, actix_web::dev::Server) {

	let tls_config = httpserver::make_tls_config(signed_cert, public_key, intermediate_cert);
	let cookie_key = httpserver::make_random_cookie_key();

	let free_session_ids = Arc::new(AtomicUsize::new(0));
	let free_ws_session_ids = Arc::new(AtomicUsize::new(0));
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
		let sys = actix::System::new("http-server");

		let data_router_addr = httpserver::data_router::DataRouter::default().start();
		let error_router_addr = httpserver::error_router::ErrorRouter::load(&db, data.clone()).unwrap().start();
		let data_router_addr_clone = data_router_addr.clone();
		let error_router_addr_clone = error_router_addr.clone();

		let web_server = HttpServer::new(move || {
			// data the webservers functions have access to
			let data = actix_web::web::Data::new(ExampleState {
				counter: Arc::new(Mutex::new(0)),
				dataserver_state: DataRouterState {
					passw_db: passw_db.clone(),
					user_db: user_db.clone(),
					data_router_addr: data_router_addr_clone.clone(),
					error_router_addr: error_router_addr_clone.clone(),
					data: data.clone(),
					sessions: sessions.clone(),
					free_session_ids: free_session_ids.clone(),
					free_ws_session_ids: free_ws_session_ids.clone(),
				},
			});
			
			App::new()
				.register_data(data)
				.wrap(IdentityService::new(
					CookieIdentityPolicy::new(&cookie_key[..])
					.domain(config::DOMAIN)
					.name("auth-cookie")
					.path("/")
					.secure(true), 
				))
				.wrap(debug_middleware::SayHi)
				.service(
					web::scope("/login")
						.service(web::resource(r"/{path}")
							.route(web::post().to(login_get_and_check::<ExampleState>))
							.route(web::get().to(login_page))
				))
				.service(web::resource("/post_data").to(new_data_post::<ExampleState>))
				.service(web::resource("/post_error").to(new_error_post::<ExampleState>))
				.service(web::resource(&format!("/{}", config::TOKEN)).to(handle_bot_message::<ExampleState>))
				//.service(web::resource("/test").to(handle_bot_message::<ExampleState>))

				.service(
					web::scope("/")
						.wrap(CheckLogin {phantom: std::marker::PhantomData::<ExampleState>})
						.service(web::resource("commands/test_state").to(test_state))
						.service(web::resource("ws/data/").to(data_router_ws_index::<ExampleState>))
						.service(web::resource("ws/error").to(error_router_ws_index::<ExampleState>))
						.service(web::resource("logout").to(logout::<ExampleState>))
						.service(web::resource("").to(index))
						.service(web::resource("plot").to(plot_data::<ExampleState>))
						.service(web::resource("list_data").to(list_data::<ExampleState>))
						//for all other urls we try to resolve to static files in the "web" dir
						.service(fs::Files::new("", "./web/"))
				)
			})
		// WARNING TLS IS NEEDED FOR THE LOGIN SYSTEM TO FUNCTION
		.bind_rustls("0.0.0.0:8443", tls_config).unwrap()
		//.bind_rustls("0.0.0.0:8080", tls_config).unwrap()
		//.bind("0.0.0.0:8080").unwrap() //without tcp use with debugging (note: https -> http, wss -> ws)
		.shutdown_timeout(5)    // shut down 5 seconds after getting the signal to shut down
		.start(); // end of App::new()


        let _ = tx.send((data_router_addr, error_router_addr, web_server));
        let _ = sys.run();
	}); //httpserver closure

    let (data_router_addr, error_router_addr, web_handle) = rx.recv().unwrap();
	(data_router_addr, error_router_addr, web_handle)
}

fn main() {
	//https://www.deviousd.duckdns.org:8080/index.html
	//only do if certs need update
	if FORCE_CERT_REGEN {
		//generate_and_sign_keys
		if let Err(error) = certificate_manager::generate_and_sign_keys(
			config::DOMAIN,
			"keys/cert.key",
			"keys/cert.cert",
			"keys/user.key",
		) {
			println!("could not auto generate certificate, error: {:?}", error)
		}
	}

	helper::setup_logging(2).expect("could not set up debugging");

	let config = sled::ConfigBuilder::new() //651ms
			.path("database".to_owned())
			.flush_every_ms(None) //do not flush to disk unless explicitly asked
			.async_io(true)
			.cache_capacity(1024 * 1024 * 32) //32 mb cache 
			.build();

	let db = sled::Db::start(config).unwrap();

	//TODO can a tree be opened multiple times?
	let mut passw_db = PasswordDatabase::from_db(&db).unwrap();
	let mut user_db = UserDatabase::from_db(&db).unwrap();
	let data = Arc::new(RwLock::new(timeseries_interface::init("data").unwrap()));
	let sessions = Arc::new(RwLock::new(HashMap::new()));

	let (data_handle, error_handle, web_handle) =
	start("keys/cert.key", "keys/cert.cert", "keys/intermediate.cert", data.clone(), passw_db.clone(), user_db.clone(), db, sessions.clone());
	println!("press: t to send test data, n: to add a new user, q to quit, a to add new dataset, o add owner to db");
	set_webhook(config::DOMAIN, config::TOKEN).unwrap();
	
	loop {
		let mut input = String::new();
		stdin().read_line(&mut input).unwrap();
		match input.as_str() {
			"t\n" => helper::send_test_data_over_http(data.clone(), 8070),
			"d\n" => helper::signal_and_append_test_data(data.clone(), &data_handle), //works
			"n\n" => helper::add_user(&mut passw_db, &mut user_db),
			"a\n" => helper::add_dataset(&mut user_db, &data),
			"o\n" => helper::add_fields_to_user(&mut user_db),
			"q\n" => break,
			_ => println!("unhandled"),
		};
	}
	println!("shutting down");
	web_handle.stop(true);
}
