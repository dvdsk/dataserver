extern crate actix;
extern crate actix_web;
extern crate actix_net;

extern crate bytes;
extern crate futures;

extern crate env_logger;
extern crate openssl;

use std::path::PathBuf;


use self::actix::*;
use self::actix::Addr;

use self::actix_web::Error as wError;
use self::actix_web::Result as wResult;
use self::actix_web::{
    fs::NamedFile, http, http::Method, server, ws, App, middleware,
    HttpRequest, HttpResponse, Responder, http::StatusCode, HttpMessage, AsyncResponder, FutureResponse,
};
use self::actix_web::middleware::identity::RequestIdentity;
use self::actix_web::middleware::identity::{CookieIdentityPolicy, IdentityService};

use self::futures::future::Future;
use self::bytes::Bytes;

use self::openssl::ssl::{SslAcceptor, SslFiletype, SslMethod};
//use futures::future::Future;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use std::path::Path;
use std::time::Instant;

mod websocket_dataserver;
mod timeseries_access;

struct WsDataSessionState {
    addr: Addr<websocket_dataserver::DataServer>,
}

type ServerHandle = self::actix::Addr<actix_net::server::Server>;
type DataHandle = self::actix::Addr<websocket_dataserver::DataServer>;

fn serve_file(req: &HttpRequest<WsDataSessionState>) -> wResult<NamedFile> {
    let file_name: PathBuf = req.match_info().query("tail")?;
    let mut path: PathBuf = PathBuf::from("web/");
    path.push(file_name);

    Ok(NamedFile::open(path)?)
}


fn index(req: &HttpRequest<WsDataSessionState>) -> String {
    format!("Hello {}", req.identity().unwrap_or("Anonymous".to_owned()))
}

fn login(req: &HttpRequest<WsDataSessionState>) -> HttpResponse {
    req.remember("user1".to_owned());
    println!("logging in");
    HttpResponse::Found().header("location", "/").finish()
}

fn logout(req: &HttpRequest<WsDataSessionState>) -> HttpResponse {
    req.forget();
    HttpResponse::Found().header("location", "/").finish()
}

fn newdata(req: &HttpRequest<WsDataSessionState>) -> FutureResponse<HttpResponse> {
    
    req.body()
        .from_err()
        .and_then(move |bytes: Bytes| {
            //timeseries_access::store_new_data(&bytes);
            println!("Body: {:?}", bytes);
            Ok(HttpResponse::Ok().status(StatusCode::ACCEPTED).finish())
        })
        .responder()
}

fn goodby(_req: &HttpRequest<WsDataSessionState>) -> impl Responder {
    "Goodby!"
}

/// do websocket handshake and start `MyWebSocket` actor
fn ws_index(r: &HttpRequest<WsDataSessionState>) -> Result<HttpResponse, wError> {
    println!("websocket connected");
    ws::start(r, WsDataSession {
            id: 0,
		},
	)
}

// store data in here, it can then be accessed using self
struct WsDataSession {
    /// unique session id
    id: usize,
}

impl Actor for WsDataSession {
	type Context = ws::WebsocketContext<Self, WsDataSessionState>;
	
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
        ctx.state().addr.do_send(websocket_dataserver::Disconnect { id: self.id });
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
							if let Ok(source) = websocket_dataserver::source_string_to_enum(v[1]){
								ctx.state().addr.do_send(websocket_dataserver::SubscribeToSource {
									id: self.id,
									source: source,
								});
							} else { warn!("unknown source: {}",v[1]); }
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
                        "/name" => {

                        }
                        _ => ctx.text(format!("!!! unknown command: {:?}", m)),
					}
				}
			},//handle other websocket commands
            ws::Message::Ping(msg) => ctx.pong(&msg),
            ws::Message::Binary(bin) => ctx.binary(bin),
            ws::Message::Close(_) => {ctx.stop();}
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

    let mut builder = SslAcceptor::mozilla_intermediate(SslMethod::tls()).unwrap();
    builder
        .set_private_key_file(private_key, SslFiletype::PEM)
        .unwrap();
    builder.set_certificate_chain_file(signed_cert).unwrap();

    let (tx, rx) = mpsc::channel();



    thread::spawn(move || {	
		// Start data server actor in separate thread
		let sys = actix::System::new("http-server");
		let data_server = Arbiter::start(|_| websocket_dataserver::DataServer::default());
        let data_server_clone = data_server.clone();
        
        let web_server = server::new(move || {       
			 // Websocket sessions state
			let state = WsDataSessionState {addr: data_server_clone.clone() };
			App::with_state(state)
			
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
            .resource("/login", |r| r.f(login))
            .resource("/logout", |r| r.f(logout))
            .resource("/", |r| r.f(index))
            //.resource(r"/newdata/{tail:.*}", |r| r.method(Method::POST).f(newdata))
			//.resource(r"/{tail:.*}", |r| r.method(Method::GET).f(serve_file))
			})
			//.bind_rustls("0.0.0.0:8080", builder).unwrap()
			.bind("0.0.0.0:8080").unwrap() //without tcp use with debugging (note: https -> http, wss -> ws)
			.shutdown_timeout(60)    // <- Set shutdown timeout to 60 seconds
			.start();

		let _ = tx.send((data_server,web_server));
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
    handle.do_send(websocket_dataserver::NewData { from: websocket_dataserver::DataSource::Light });
        println!("send signal there is new data");
        //.timeout(Duration::from_secs(5)); 
}
