#[macro_use] extern crate log;

mod certificate_manager;
mod httpserver;

use std::path::Path;
	
use std::io::{stdin, stdout, Read, Write};
pub fn pause() {
    let mut stdout = stdout();
    stdout
        .write(b"Press Enter to halt servers and quit...")
        .unwrap();
    stdout.flush().unwrap();
    stdin().read(&mut [0]).unwrap();
}
	
fn main() {

	
    //https://www.deviousd.duckdns.org:8080/index.html
	//only do if certs need update
    ////generate_and_sign_keys
    //if let Err(error) = certificate_manager::generate_and_sign_keys(
        //"deviousd.duckdns.org",
        //Path::new("keys/cert.key"),
        //Path::new("keys/cert.cert"),
        //Path::new("keys/user.key"),
    //) {
        //println!("could not auto generate certificate, error: {:?}", error)
    //}

    let (data_handle, web_handle) = httpserver::start( Path::new("keys/cert.key"), Path::new("keys/cert.cert") );
    pause();    
    httpserver::send_newdata(data_handle);
    //httpserver::send_test(data_handle);
    pause();
    httpserver::stop(web_handle);
}
