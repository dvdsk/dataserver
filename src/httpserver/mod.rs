extern crate actix;
extern crate actix_net;
extern crate actix_web;
extern crate actix_web_httpauth;

extern crate bytes;
extern crate futures;

extern crate env_logger;
extern crate rustls;

extern crate minimal_timeseries;

use std::path::PathBuf;

use self::actix::Addr;
use self::actix::*;

use self::actix_web::http::header::Header;
use self::actix_web::middleware::identity::RequestIdentity;
use self::actix_web::middleware::identity::{CookieIdentityPolicy, IdentityService};
use self::actix_web::Error as wError;
use self::actix_web::Result as wResult;
use self::actix_web::{
	fs::NamedFile, http, http::Method, http::StatusCode, middleware, server, ws, App, State,
	AsyncResponder, Form, FutureResponse, HttpMessage, HttpRequest, HttpResponse, Responder,
};

use self::actix_web_httpauth::headers::www_authenticate::WWWAuthenticate;

use self::actix_web_httpauth::headers::authorization::{Authorization, Basic};
use self::actix_web_httpauth::headers::www_authenticate::basic::Basic as Basic_auth_header;

use self::bytes::Bytes;
use self::futures::future::Future;

use self::rustls::internal::pemfile::{certs, pkcs8_private_keys, rsa_private_keys};
use self::rustls::{NoClientAuth, ServerConfig};

use std::fs::File;
use std::io::BufReader;

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use self::minimal_timeseries::Timeseries;

mod timeseries_access;
mod websocket_dataserver;

struct SessionState {
	addr: Addr<websocket_dataserver::DataServer>,
	data: HashMap<u16, Timeseries>,
}

#[derive(Deserialize)]
struct loginData {
	u: String,
	p: String,
}

type ServerHandle = self::actix::Addr<actix_net::server::Server>;
type DataHandle = self::actix::Addr<websocket_dataserver::DataServer>;

fn serve_file(req: &HttpRequest<SessionState>) -> wResult<NamedFile> {
	let file_name: String = req.match_info().query("tail")?;

	let mut path: PathBuf = PathBuf::from("web/");
	path.push(file_name);
	Ok(NamedFile::open(path)?)
}

fn index(req: &HttpRequest<SessionState>) -> String {
	format!("Hello {}", req.identity().unwrap_or("Anonymous".to_owned()))
}

fn logout(req: &HttpRequest<SessionState>) -> HttpResponse {
	req.forget();
	HttpResponse::Found().header("location", "/").finish()
}

pub struct CheckLogin;
impl<S> middleware::Middleware<S> for CheckLogin {
	// We only need to hook into the `start` for this middleware.
	fn start(&self, req: &HttpRequest<S>) -> wResult<middleware::Started> {
		if let Some(id) = req.identity() {
			return Ok(middleware::Started::Done);
		} else {
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
}

fn login_page(req: &HttpRequest<SessionState>) -> HttpResponse {
    println!("hi");
	let page = include_str!("static_webpages/login.html");
	HttpResponse::Ok().header(http::header::CONTENT_TYPE, "text/html; charset=utf-8").body(page)
}

//fn login_get_and_check(req: &HttpRequest<SessionState>) -> wResult<String> {
    //println!("let me check that");
    //println!("{:?}",req.match_info() );
	//let ivalue: isize = req.match_info().query("u")?;
	//println!("{:?}", ivalue);
	//Ok(format!("isuze value: {:?}", ivalue))
//}

/// State and POST Params
fn login_get_and_check(
    (state, params): (State<SessionState>, Form<loginData>),
) -> wResult<HttpResponse> {
    
    println!("{:?}",params.u);
    
    Ok(HttpResponse::build(http::StatusCode::OK)
        .content_type("text/plain")
        .body(format!(
            "Your name is {}, and in AppState I have foo: {}",
            params.u, params.p
        )))
}

fn newdata(req: &HttpRequest<SessionState>) -> FutureResponse<HttpResponse> {
	println!("funct start");

	req.body()
		.from_err()
		.and_then(move |bytes: Bytes| {
			timeseries_access::store_new_data(&bytes);
			println!("Body: {:?}", bytes);
			Ok(HttpResponse::Ok().status(StatusCode::ACCEPTED).finish())
		}).responder()
}

fn goodby(_req: &HttpRequest<SessionState>) -> impl Responder {
	"Goodby!"
}

/// do websocket handshake and start `MyWebSocket` actor
fn ws_index(r: &HttpRequest<SessionState>) -> Result<HttpResponse, wError> {
	println!("websocket connected");
	ws::start(r, WsDataSession { id: 0 })
}

// store data in here, it can then be accessed using self
struct WsDataSession {
	/// unique session id
	id: usize,
}

impl Actor for WsDataSession {
	type Context = ws::WebsocketContext<Self, SessionState>;

	fn started(&mut self, ctx: &mut Self::Context) {
		// register self in chat server. `AsyncContext::wait` register
		// future within context, but context waits until this future resolves
		// before processing any other events.
		// HttpContext::state() is instance of WsChatSessionState, state is shared
		// across all routes within application

		println!("TEST");

		let addr = ctx.address();
		ctx.state()
            .addr
            .send(websocket_dataserver::Connect {
                addr: addr.recipient(),
            })
            //wait for response
            .into_actor(self)
            //process response in closure
            .then(|res, act, ctx| {
                match res {
                    Ok(res) => act.id = res,
                    // something is wrong with chat server
                    _ => ctx.stop(),
                }
                fut::ok(())
            })
            .wait(ctx);
	}

	fn stopping(&mut self, ctx: &mut Self::Context) -> Running {
		// notify chat server
		ctx.state()
			.addr
			.do_send(websocket_dataserver::Disconnect { id: self.id });
		Running::Stop
	}
}

/// send messages to server if requested by dataserver
impl Handler<websocket_dataserver::clientMessage> for WsDataSession {
	type Result = ();

	fn handle(&mut self, msg: websocket_dataserver::clientMessage, ctx: &mut Self::Context) {
		println!("websocket");
		ctx.text(msg.0);
	}
}

/// Handler for `ws::Message`
impl StreamHandler<ws::Message, ws::ProtocolError> for WsDataSession {
	fn handle(&mut self, msg: ws::Message, ctx: &mut Self::Context) {
		// process websocket messages
		println!("WS: {:?}", msg);
		match msg {
			ws::Message::Text(text) => {
				let m = text.trim();
				if m.starts_with('/') {
					let v: Vec<&str> = m.splitn(2, ' ').collect();
					match v[0] {
						"/Sub" => {
							if let Ok(source) = websocket_dataserver::source_string_to_enum(v[1]) {
								ctx.state()
									.addr
									.do_send(websocket_dataserver::SubscribeToSource {
										id: self.id,
										source: source,
									});
							} else {
								warn!("unknown source: {}", v[1]);
							}
						}
						"/join" => {
							//if v.len() == 2 {
							//self.room = v[1].to_owned();
							//ctx.state().addr.do_send(server::Join {
							//id: self.id,
							//name: self.room.clone(),
							//});

							//ctx.text("joined");
							//} else {
							//ctx.text("!!! room name is required");
							//}
						}
						"/name" => {}
						_ => ctx.text(format!("!!! unknown command: {:?}", m)),
					}
				}
			} //handle other websocket commands
			ws::Message::Ping(msg) => ctx.pong(&msg),
			ws::Message::Binary(bin) => ctx.binary(bin),
			ws::Message::Close(_) => {
				ctx.stop();
			}
			_ => (),
		}
	}
}

pub fn start(signed_cert: &Path, private_key: &Path) -> (DataHandle, ServerHandle) {
	// load ssl keys

	if ::std::env::var("RUST_LOG").is_err() {
		::std::env::set_var("RUST_LOG", "actix_web=trace");
	}
	env_logger::init();

	let mut config = ServerConfig::new(NoClientAuth::new());
	let cert_file = &mut BufReader::new(File::open(signed_cert).unwrap());
	let key_file = &mut BufReader::new(File::open(private_key).unwrap());
	let cert_chain = certs(cert_file).unwrap();
	let mut key = pkcs8_private_keys(key_file).unwrap();
	config
		.set_single_cert(cert_chain, key.pop().unwrap())
		.unwrap();

	let (tx, rx) = mpsc::channel();

	thread::spawn(move || {
		// Start data server actor in separate thread
		let sys = actix::System::new("http-server");
		let data_server = Arbiter::start(|_| websocket_dataserver::DataServer::default());
		let data_server_clone = data_server.clone();

		let web_server = server::new(move || {
			 // Websocket sessions state
			let state = SessionState {
                addr: data_server_clone.clone(),
                data: timeseries_access::init(PathBuf::from("data")).unwrap(), 
            };
			App::with_state(state)
			.middleware(CheckLogin)
            .middleware(IdentityService::new(
                CookieIdentityPolicy::new(&[0; 32])
                    .name("plantmonitor_session")
                    .secure(true),
            ))
                // websocket route
                // note some browsers need already existing http connection to 
                // this server for the upgrade to wss to work
                .resource("/ws/", |r| r.method(http::Method::GET).f(ws_index))
                .resource("/goodby.html", |r| r.f(goodby)) 
                .resource("/logout", |r| r.f(logout))
                .resource("/", |r| r.f(index))
                .resource(r"/newdata", |r| r.method(Method::POST).f(newdata))
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

pub fn send_newdata(handle: DataHandle) {
	handle.do_send(websocket_dataserver::NewData {
		from: websocket_dataserver::DataSource::Light,
	});
	println!("send signal there is new data");
	//.timeout(Duration::from_secs(5));
}
