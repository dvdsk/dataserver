extern crate openssl;
extern crate ws;

use self::ws::util::TcpStream;
use self::ws::Error as wsError;
use self::ws::Result as wsResult;
use self::ws::{CloseCode, Handler, Handshake, Message, Sender};

use std::fs::File;

use std::io::{Error, Read};
use std::rc::Rc;
use std::thread;

use self::openssl::pkey::PKey;
use self::openssl::ssl::{SslAcceptor, SslMethod, SslStream};
use self::openssl::x509::X509;

struct Server {
    out: Sender,
    ssl: Rc<SslAcceptor>,
}

impl Handler for Server {
    fn upgrade_ssl_server(&mut self, sock: TcpStream) -> wsResult<SslStream<TcpStream>> {
        println!("trying to ssl");
        println!("{:?}",self.ssl.accept(sock));
        self.ssl.accept(sock).map_err(From::from)
    }

    fn on_open(&mut self, _: Handshake) -> wsResult<()> {
        // We have a new connection, so we increment the connection counter
        println!("user connected to webserver");
        Ok(())
    }

    fn on_message(&mut self, msg: Message) -> wsResult<()> {
        // Tell the user the current count
        println!("user send message to webserver");

        // Echo the message back
        self.out.send(msg)
    }

    fn on_close(&mut self, code: CloseCode, reason: &str) {
        match code {
            CloseCode::Normal => println!("The client is done with the connection."),
            CloseCode::Away => println!("The client is leaving the site."),
            CloseCode::Abnormal => {
                println!("Closing handshake failed! Unable to obtain closing status from client.")
            }
            _ => println!("The client encountered an error: {}", reason),
        }

        // The connection is going down
    }

    fn on_error(&mut self, err: wsError) {
        println!("The server encountered an error: {:?}", err);
    }
}

pub fn start() {
    let cert = {
        let data = read_file("cert.pem").unwrap();
        X509::from_pem(data.as_ref()).unwrap()
    };

    let pkey = {
        let data = read_file("key.pem").unwrap();
        PKey::private_key_from_pem(data.as_ref()).unwrap()
    };

    thread::spawn(move || {
        let acceptor = Rc::new({
            let mut builder = SslAcceptor::mozilla_intermediate(SslMethod::tls()).unwrap();
            builder.set_private_key(&pkey).unwrap();
            builder.set_certificate(&cert).unwrap();
            builder.build()
        });

        ws::Builder::new()
            .with_settings(ws::Settings {
                encrypt_server: true,
                ..ws::Settings::default()
            }).build(|out: ws::Sender| Server {
                out: out,
                ssl: acceptor.clone(),
            }).unwrap()
            .listen("0.0.0.0:3012")
            .unwrap();
    });
}

fn read_file(name: &str) -> Result<Vec<u8>, Error> {
    let mut file = File::open(name)?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    Ok(buf)
}
