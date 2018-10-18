#[macro_use] extern crate log;

mod certificate_manager;
mod httpserver;

use std::path::Path;
	
use std::io::{stdin, stdout, Read, Write};
pub fn pause() {
    let mut stdout = stdout();
    stdout
        .write(b"Press Enter to halt servers and quit...\n")
        .unwrap();
    stdout.flush().unwrap();
    stdin().read(&mut [0]).unwrap();
}
	
fn main() {

	
    //https://www.deviousd.duckdns.org:8080/index.html
	//only do if certs need update
    if false {
        //generate_and_sign_keys
        if let Err(error) = certificate_manager::generate_and_sign_keys(
            "deviousd.duckdns.org",
            Path::new("keys/cert.key"),
            Path::new("keys/cert.cert"),
            Path::new("keys/user.key"),
        ) {
            println!("could not auto generate certificate, error: {:?}", error)
        }
    }

    let (data_handle, web_handle) = httpserver::start( Path::new("keys/cert.key"), Path::new("keys/cert.cert") );
    pause();    
    httpserver::send_newdata(data_handle);
    //httpserver::send_test(data_handle);
    pause();
    httpserver::stop(web_handle);
}


#[cfg(test)]
mod tests {
    use super::*;    
    extern crate reqwest;
    extern crate byteorder;
    use self::byteorder::{WriteBytesExt, NativeEndian, LittleEndian};
    
    #[test]
    fn put_new_data() {
        
        let (data_handle, web_handle) = httpserver::start( Path::new("keys/cert.key"), Path::new("keys/cert.cert") );     
        let client = reqwest::Client::builder().danger_accept_invalid_certs(true).build().unwrap();

        let node_id: u16 = 2233;        
        let temp: f32 = 20.34;
        let humidity: f32 = 53.12;

        let mut data_string: Vec<u8> = Vec::new();
        data_string.write_u16::<NativeEndian>(node_id).unwrap();
        data_string.write_u16::<NativeEndian>(((temp+20.)*100.) as u16).unwrap();
        data_string.write_u16::<NativeEndian>((humidity*100.) as u16).unwrap();
        
        let res = client.post("https://www.deviousd.duckdns.org:8080/newdata")
                 .body(data_string)
                 .send().unwrap();
        println!("res: {:?}",res);
        pause();        
        httpserver::stop(web_handle);   
    }
}
