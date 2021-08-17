use actix_rt::net::{TcpStream, TcpListener};
use std::net::SocketAddr;

use std::sync::{Arc, RwLock};

use crate::data_store::Data;
use crate::database::{AlarmDatabase, PasswordDatabase, UserDatabase, UserLookup};

mod data;
// mod user;

fn stream_handler(
    mut socket: TcpStream,
	data: &Arc<RwLock<Data>>,
	passw_db: &mut PasswordDatabase,
	user_db: &mut UserDatabase,
	alarm_db: &AlarmDatabase,
	lookup: &UserLookup,
) {
    
	loop {
	}
}

pub async fn main(
    port: u16,
    data: Arc<RwLock<Data>>,
    mut passw_db: PasswordDatabase,
    mut user_db: UserDatabase,
    alarm_db: AlarmDatabase,
    lookup: UserLookup,
) {
    // admin interface only availible from 127.0.0.1 (localhost)
    let addr = SocketAddr::from(([127,0,0,1], port));
    let mut listener = TcpListener::bind(addr).await.unwrap();

    loop {
        let (socket, _) = listener.accept().await.unwrap();
        stream_handler(socket, &data, &mut passw_db, &mut user_db, &alarm_db, &lookup);
    }
}
