extern crate acme_client;

extern crate actix_web;
extern crate untrusted;
extern crate webpki;
extern crate webpki_roots;

extern crate actix;
extern crate actix_net;

use self::webpki_roots::TLS_SERVER_ROOTS;

use self::acme_client::error::Error as aError;
use self::acme_client::Directory;
//use self::acme_client::LETSENCRYPT_INTERMEDIATE_CERT_URL;

use actix_web::Result as wResult;
use actix_web::{http, HttpServer, App, HttpRequest, web};
use actix_files as fs;
use std::sync::mpsc;
use std::thread;

use std::env;
use std::fs::create_dir;
use std::fs::remove_dir_all;
use std::fs::File;
use std::io;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

fn certificate_valid(_signed_cert: &Path, _private_key: &Path) -> bool {
	//if signed_cert.exists() && if private_key.exists(){

	////static ALL_SIGALGS: &'static [&'static webpki::SignatureAlgorithm] = &[
	////&webpki::ECDSA_P256_SHA256,
	////&webpki::ECDSA_P256_SHA384,
	////&webpki::ECDSA_P384_SHA256,
	////&webpki::ECDSA_P384_SHA384,
	////&webpki::RSA_PKCS1_2048_8192_SHA1,
	////&webpki::RSA_PKCS1_2048_8192_SHA256,
	////&webpki::RSA_PKCS1_2048_8192_SHA384,
	////&webpki::RSA_PKCS1_2048_8192_SHA512,
	////&webpki::RSA_PKCS1_3072_8192_SHA384
	////];

	////cert.verify_is_valid_tls_server_cert(
	////ALL_SIGALGS, &anchors,
	////&inter_vec, time)
	////.is_err()
	//true
	//} else {
	//false
	//}

	true
}

/* Checks we can verify netflix's cert chain.  This is notable
 * because they're rooted at a Verisign v1 root. */
pub fn netflix(signed_cert: &Path, intermediate_cert: &Path) {
	static ALL_SIGALGS: &'static [&'static webpki::SignatureAlgorithm] = &[
		&webpki::ECDSA_P256_SHA256,
		&webpki::ECDSA_P256_SHA384,
		&webpki::ECDSA_P384_SHA256,
		&webpki::ECDSA_P384_SHA384,
		&webpki::RSA_PKCS1_2048_8192_SHA1,
		&webpki::RSA_PKCS1_2048_8192_SHA256,
		&webpki::RSA_PKCS1_2048_8192_SHA384,
		&webpki::RSA_PKCS1_2048_8192_SHA512,
		&webpki::RSA_PKCS1_3072_8192_SHA384,
	];

	//let ee = include_bytes!("netflix/ee.der");
	//let inter = include_bytes!("netflix/inter.der");

	let mut f = File::open(signed_cert).unwrap();
	let mut buffer = Vec::new();
	f.read_to_end(&mut buffer).unwrap();

	//let ee_input = untrusted::Input::from(ee);
	let ee_input = untrusted::Input::from(buffer.as_slice());

	let mut f = File::open(intermediate_cert).unwrap();
	let mut buffer = Vec::new();
	f.read_to_end(&mut buffer).unwrap();
	let inter_vec = vec![untrusted::Input::from(buffer.as_slice())];

	//let inter_vec = vec![ untrusted::Input::from(inter) ];

	let time = webpki::Time::from_seconds_since_unix_epoch(1492441716);

	let cert = webpki::EndEntityCert::from(ee_input).unwrap();
	let outcome =
		cert.verify_is_valid_tls_server_cert(ALL_SIGALGS, &TLS_SERVER_ROOTS, &inter_vec, time);
	println!("outcome: {:?}", outcome);
}

fn am_root() -> bool {
	match env::var("USER") {
		Ok(val) => val == "root",
		Err(_e) => false,
	}
}

fn get_port() -> Result<u32, ()> {
	if am_root() {
		//ask if port 80 has been forwarded
		println!("has the external (WAN) port 80 been forwarded to this machines port 80? (y/N)");
		let mut input_text = String::new();
		io::stdin()
			.read_line(&mut input_text)
			.expect("failed to read from stdin");

		let trimmed = input_text.trim();
		if trimmed == "y" {
			Ok(80)
		} else {
			Err(())
		}
	} else {
		println!("please input a internal (LAN) port to which the external (WAN) port 80 has been forwarded:");
		let mut input_text = String::new();
		io::stdin()
			.read_line(&mut input_text)
			.expect("failed to read from stdin");

		let trimmed = input_text.trim();
		match trimmed.parse::<u32>() {
			Ok(i) => Ok(i),
			Err(..) => {
				println!("that was not an integer: {}", trimmed);
				Err(())
			}
		}
	}
}

fn print(_req: &HttpRequest) -> &'static str {
    "Hello world!"
}

type ServerHandle = self::actix::Addr<actix_net::server::Server>;
pub fn host_server() -> Result<ServerHandle, ()> {
	if let Ok(port) = get_port() {
		let socket = format!("0.0.0.0:{}", port);
		println!("socket :{}", socket);

		let (tx, rx) = mpsc::channel();
		thread::spawn(move || {
			let sys = actix::System::new("http-server");
			let addr = HttpServer::new(|| App::new()
			//handle only requests for certificate challanges
			.service(web::resource("/").to(print))
			.service(fs::Files::new("/.well-known/acme-challenge/", "."))
			)
			.bind(&socket).expect(&format!("Can not bind to {}",socket))
			.shutdown_timeout(5)    // <- Set shutdown timeout to 5 seconds
			.start();

			let _ = tx.send(addr);
			let _ = sys.run();
		});

		let handle = rx.recv().unwrap();
		Ok(handle)
	} else {
		Err(())
	}
}

fn stop_server(handle: ServerHandle) {
	let _ = handle
		.send(HttpServer::StopServer { graceful: true })
		.timeout(Duration::from_secs(5)); // <- Send `StopServer` message to server.
}

// FIXME
fn make_domain_list(domain: &str) -> (String, String) {
	let mut domain = domain.to_owned();

	if domain.starts_with("www.") {
		let without_www = domain.split_off(4);
		(domain, without_www)
	} else {
		let www = "www.".to_owned();
		(www + &domain, domain)
	}
}

pub fn generate_and_sign_keys<T: AsRef<Path>>(
	domain: &str,
	signed_cert: T,
	private_key: T,
	user_private_key: T,
) -> Result<(), aError> {
	let signed_cert = signed_cert.as_ref();
	let private_key =	private_key.as_ref();
	let user_private_key =	user_private_key.as_ref();

	let (a, b) = make_domain_list(domain);
	let domains = [a.as_str(), b.as_str()];
	let directory = Directory::lets_encrypt().unwrap();

	let account = if user_private_key.exists() {
		directory
			.account_registration()
			.pkey_from_file(&user_private_key)
			.unwrap()
			.register()
			.unwrap()
	} else {
		let account = directory.account_registration().register().unwrap();
		//store newly generated private key
		account.save_private_key(&user_private_key).unwrap();
		account
	};

	// Create a identifier authorization for example.com
	create_dir(".tmp/www");
	//host server with key saved above
	let server = host_server().expect("needs to be ran as root");

	//enable to halt signing process and check if signing request server is reachable
	// loop {
	// 	let mut input = String::new();
	// 	std::io::stdin().read_line(&mut input).unwrap();
	// 	match input.as_str() {
	// 		"q\n" => break,
	// 		_ => println!("unhandled"),
	// 	};
	// }

	for domain in domains.iter() {
		let authorization = account.authorization(domain).unwrap();

		// Validate ownership of example.com with http challenge
		let http_challenge = authorization
			.get_http_challenge()
			.ok_or("HTTP challenge not found")
			.unwrap();

		http_challenge.save_key_authorization(".tmp/www").unwrap();
		http_challenge.validate().unwrap();
	}

	//done, we can shut this server down
	stop_server(server);
	//clean up challange dir
	remove_dir_all(".tmp/www")?;

	//this wil generate a key and csr (certificate signing request)
	//then send the csr off for signing
	let cert = account
		.certificate_signer(&domains)
		.sign_certificate()
		.unwrap();

	cert.save_signed_certificate(&signed_cert).unwrap(); //should end in .pem
	cert.save_private_key(&private_key).unwrap(); //should end in .key
	cert.save_intermediate_certificate(None, "intermediate.cert")
		.unwrap();
	Ok(())
}

//#[test]
//pub fn netflix_test() {
	//netflix(Path::new("tests/ee.der"), Path::new("tests/inter.der"));
//}
