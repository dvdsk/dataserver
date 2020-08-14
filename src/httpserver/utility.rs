use rand::{rngs::StdRng, Rng, SeedableRng};
use rustls::internal::pemfile::{certs, pkcs8_private_keys};
use rustls::{NoClientAuth, ServerConfig};

use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum Error {
	NoKeyFound,
	NoCertFound,
}

pub fn make_random_cookie_key() -> [u8; 32] {
	let mut cookie_private_key = [0u8; 32];
	let mut rng = StdRng::from_entropy();
	rng.fill(&mut cookie_private_key[..]);
	cookie_private_key
}

fn get_key_and_cert(domain: &str, dir: &Path) -> Result<(PathBuf, PathBuf), Error> {
	let mut cert_path = Err(Error::NoCertFound);
	let mut key_path = Err(Error::NoKeyFound);
	let domain = domain.replace(".", "_");
	for path in fs::read_dir(dir)
		.unwrap()
		.filter_map(Result::ok)
		.map(|entry| entry.path())
	{
		if let Some(stem) = path.file_stem().map(|s| s.to_str()).flatten() {
			if !stem.contains(&domain) {
				continue;
			}
			if let Some(ext) = path.extension().map(|s| s.to_str()).flatten() {
				match ext {
					"key" => key_path = Ok(path),
					"crt" => cert_path = Ok(path),
					_ => continue,
				}
			}
		}
	}

	Ok((key_path?, cert_path?))
}

pub fn make_tls_config(domain: &str, key_dir: &Path) -> Result<rustls::ServerConfig, Error> {
	//find cert and key
	let (key_path, cert_path) = get_key_and_cert(domain, key_dir)?;

	let mut tls_config = ServerConfig::new(NoClientAuth::new());
	let cert_file = &mut BufReader::new(
		fs::File::open(&cert_path)
			.expect(&format!("could not open certificate file: {:?}", cert_path)),
	);
	let key_file = &mut BufReader::new(
		fs::File::open(&key_path).expect(&format!("could not open key file: {:?}", key_path)),
	);

	let cert_chain = certs(cert_file).unwrap();
	let mut key = pkcs8_private_keys(key_file).unwrap();

	tls_config
		.set_single_cert(cert_chain, key.pop().unwrap())
		.unwrap();
	Ok(tls_config)
}
