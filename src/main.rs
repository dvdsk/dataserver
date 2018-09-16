mod certificate_manager;
mod httpserver;
use httpserver::pause;

use std::path::Path;

fn main() {

	
    //https://www.deviousd.duckdns.org:8080/index.html

    //generate_and_sign_keys
    if let Err(error) = certificate_manager::generate_and_sign_keys(
        "deviousd.duckdns.org",
        Path::new("keys/cert.key"),
        Path::new("keys/cert.cert"),
        Path::new("keys/user.key"),
    ) {
        println!("could not auto generate certificate, error: {:?}", error)
    }

	//certificate_manager::host_server();

    //let handle = httpserver::start();
    pause();
    //httpserver::stop(handle);
}
