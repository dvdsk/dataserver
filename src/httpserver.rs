extern crate actix;
extern crate actix_web;
extern crate env_logger;
extern crate openssl;

use std::path::PathBuf;

use self::actix::*;
use self::actix_web::Error as wError;
use self::actix_web::Result as wResult;
use self::actix_web::{
    fs::NamedFile, http, http::Method, server, ws, App, HttpRequest, HttpResponse, Responder,
};

use self::openssl::ssl::{SslAcceptor, SslFiletype, SslMethod};
//use futures::future::Future;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use std::path::Path;

type ServerHandle = self::actix::Addr<self::actix_web::server::Server>;

fn index(req: &HttpRequest) -> wResult<NamedFile> {
    let file_name: PathBuf = req.match_info().query("tail")?;
    let mut path: PathBuf = PathBuf::from("web/");
    path.push(file_name);

    Ok(NamedFile::open(path)?)
}

fn goodby(_req: &HttpRequest) -> impl Responder {
    "Goodby!"
}

/// do websocket handshake and start `MyWebSocket` actor
fn ws_index(r: &HttpRequest) -> Result<HttpResponse, wError> {
    println!("websocket connected");
    ws::start(r, MyWebSocket)
}

/// websocket connection is long running connection, it easier
/// to handle with an actor
struct MyWebSocket;

impl Actor for MyWebSocket {
    type Context = ws::WebsocketContext<Self>;
}

/// Handler for `ws::Message`
impl StreamHandler<ws::Message, ws::ProtocolError> for MyWebSocket {
    fn handle(&mut self, msg: ws::Message, ctx: &mut Self::Context) {
        // process websocket messages
        println!("WS: {:?}", msg);
        match msg {
            ws::Message::Ping(msg) => ctx.pong(&msg),
            ws::Message::Text(text) => ctx.text(text),
            ws::Message::Binary(bin) => ctx.binary(bin),
            ws::Message::Close(_) => {
                ctx.stop();
            }
            _ => (),
        }
    }
}

pub fn start(signed_cert: &Path, private_key: &Path) -> ServerHandle {
    // load ssl keys

    if ::std::env::var("RUST_LOG").is_err() {
        ::std::env::set_var("RUST_LOG", "actix_web=info");
    }
    env_logger::init();

    let mut builder = SslAcceptor::mozilla_intermediate(SslMethod::tls()).unwrap();
    builder
        .set_private_key_file(private_key, SslFiletype::PEM)
        .unwrap();
    builder.set_certificate_chain_file(signed_cert).unwrap();

    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let sys = actix::System::new("http-server");
        let addr = server::new(|| App::new()
        // websocket route
        .resource("/ws/", |r| r.method(http::Method::GET).f(ws_index))
        .resource(r"/{tail:.*}", |r| r.method(Method::GET).f(index))
        .resource("/goodby.html", |r| r.f(goodby)) 
        )
		.bind_ssl("0.0.0.0:8060", builder).unwrap()
        //.bind("0.0.0.0:8080").unwrap() //without tcp use with debugging (note: https -> http, wss -> ws)
		.shutdown_timeout(60)    // <- Set shutdown timeout to 60 seconds
		.start();

        let _ = tx.send(addr);
        let _ = sys.run();
    });

    let handle = rx.recv().unwrap();
    handle
}

pub fn stop(handle: ServerHandle) {
    let _ = handle
        .send(server::StopServer { graceful: true })
        .timeout(Duration::from_secs(5)); // <- Send `StopServer` message to server.
}

use std::io::{stdin, stdout, Read, Write};
pub fn pause() {
    let mut stdout = stdout();
    stdout
        .write(b"Press Enter to halt servers and quit...")
        .unwrap();
    stdout.flush().unwrap();
    stdin().read(&mut [0]).unwrap();
}
