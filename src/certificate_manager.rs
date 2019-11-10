use acme_client::error::Error as aError;
use acme_client::Directory;
//use self::acme_client::LETSENCRYPT_INTERMEDIATE_CERT_URL;

use actix_web::{HttpServer, App, Responder, HttpResponse};
use actix_files as fs;
use std::sync::mpsc;
use std::thread;

use std::env;
use std::fs::create_dir_all;
use std::fs::remove_dir_all;
use std::io;
use std::path::Path;

fn certificate_valid(_signed_cert: &Path, _private_key: &Path) -> bool {
	unimplemented!();
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

fn index() -> impl Responder {
    HttpResponse::Ok().body("Hello world!, the certificate challange server is up")
}

//handles only requests for certificate challanges
pub fn host_server() -> Result<actix_web::dev::Server, ()> {
	if let Ok(port) = get_port() {
		let socket = format!("0.0.0.0:{}", port);
		println!("socket :{}", socket);

		let (tx, rx) = mpsc::channel();
		thread::spawn(move || {
        	let sys = actix_rt::System::new("http-server");

			let addr = HttpServer::new(|| 
				App::new()
				.route("/", actix_web::web::get().to(index))
				.service(fs::Files::new("/.well-known/acme-challenge", "./.tmp/www/.well-known/acme-challenge"))
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
	println!("generating and signing new certificate and private key");
	let signed_cert = signed_cert.as_ref();
	let private_key = private_key.as_ref();
	let user_private_key = user_private_key.as_ref();

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
	if !Path::new(".tmp/www").exists(){
		create_dir_all(".tmp/www").unwrap();
	}
	//host server with key saved above
	let server = host_server().expect("needs to be ran as root");

	//enable to halt signing process and check if signing request server is reachable
	println!("check if the server is reachable and or press enter to continue");
	let mut input = String::new();
	std::io::stdin().read_line(&mut input).unwrap();

	for domain in domains.iter() {
		let authorization = account.authorization(domain).unwrap();

		// Validate ownership of example.com with http challenge
		let http_challenge = authorization
			.get_http_challenge()
			.ok_or("HTTP challenge not found")
			.unwrap();

		http_challenge.save_key_authorization(".tmp/www").unwrap();
		http_challenge.validate().unwrap();

		//thread::sleep(Duration::from_secs(40));
	}

	//done, we can shut this server down non gracefully
	server.stop(false);
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