extern crate actix;
extern crate actix_web;
extern crate openssl;

//use self::actix::System;
use std::path::PathBuf;
use self::actix_web::{server, App, HttpRequest, Responder, fs::NamedFile, http::Method};
use self::actix_web::Result as wResult;
use self::openssl::ssl::{SslAcceptor, SslFiletype, SslMethod};
//use futures::future::Future;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

type ServerHandle = self::actix::Addr<self::actix_web::server::Server>;

fn index(req: &HttpRequest) -> wResult<NamedFile> {
	let file_name: PathBuf = req.match_info().query("tail")?;
    let mut path: PathBuf = PathBuf::from("web/");
    path.push(file_name );
    
    Ok(NamedFile::open(path)?)
}

fn goodby(_req: &HttpRequest) -> impl Responder {
    "Goodby!"
}

pub fn start() -> ServerHandle {
    // load ssl keys
    let mut builder = SslAcceptor::mozilla_intermediate(SslMethod::tls()).unwrap();
    builder
        .set_private_key_file("key.pem", SslFiletype::PEM)
        .unwrap();

    builder.set_certificate_chain_file("cert.pem").unwrap();

    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let sys = actix::System::new("http-server");
        let addr = server::new(|| App::new()
        .resource(r"/{tail:.*}", |r| r.method(Method::GET).f(index))
        .resource("/goodby.html", |r| r.f(goodby)) 
        )
		.bind_ssl("0.0.0.0:8080", builder).unwrap()
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
