extern crate websocket;
extern crate native_tls;

use std::thread;
use std::io::Read;
use std::fs::File;

use self::websocket::Message;
use self::websocket::sync::Server;
//use self::websocket::native_tls::{Identity, TlsAcceptor, TlsStream};
use self::native_tls::{Pkcs12, TlsAcceptor};

pub fn start(){
	
	let mut file = File::open("identity.pfx").unwrap();
	let mut pkcs12 = vec![];
	file.read_to_end(&mut pkcs12).unwrap();
	let pkcs12 = Pkcs12::from_der(&pkcs12, "").unwrap();

	let acceptor = TlsAcceptor::builder(pkcs12).unwrap().build().unwrap();

	let server = Server::bind_secure("0.0.0.0:3012", acceptor).unwrap();

	thread::spawn(move || {
		println!("testy");
		for connection in server {
			println!("client connected to websocket, error: {:?}", connection.is_err());
			// Spawn a new thread for each connection.
			//thread::spawn(move || {
					//let mut client = connection.accept().unwrap();
					//let message = Message::text("Hello, client!");
					//let _ = client.send_message(&message);

					//// ...
			//});
		}
	});
}
