use rustls::internal::pemfile::{certs, pkcs8_private_keys};
use rustls::{NoClientAuth, ServerConfig};
use rand::FromEntropy;
use rand::Rng;

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

pub fn make_random_cookie_key() -> [u8; 32] {
	let mut cookie_private_key = [0u8; 32];
	let mut rng = rand::StdRng::from_entropy();
	rng.fill(&mut cookie_private_key[..]);
	cookie_private_key
}

pub fn make_tls_config<P: AsRef<Path>+std::fmt::Display>(cert_path: P, key_path: P, 
    intermediate_cert_path: P) 
-> rustls::ServerConfig{

	dbg!();
	let mut tls_config = ServerConfig::new(NoClientAuth::new());
	let cert_file = &mut BufReader::new(File::open(&cert_path)
		.expect(&format!("could not open certificate file: {}", cert_path)));
	let intermediate_file = &mut BufReader::new(File::open(&intermediate_cert_path)
		.expect(&format!("could not open intermediate certificate file: {}", intermediate_cert_path)));
	let key_file = &mut BufReader::new(File::open(&key_path)
		.expect(&format!("could not open key file: {}", key_path)));

	let mut cert_chain = certs(cert_file).unwrap();
	cert_chain.push(certs(intermediate_file).unwrap().pop().unwrap());

	let mut key = pkcs8_private_keys(key_file).unwrap();

	tls_config
		.set_single_cert(cert_chain, key.pop().unwrap())
		.unwrap();
	tls_config
}