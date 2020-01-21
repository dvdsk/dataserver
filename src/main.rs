//#[cfg(test)] //TODO: adapt tests and re-enable
//mod test;

mod certificate_manager;
mod debug_middleware;
mod error;
mod bot;
mod databases;
mod httpserver;
mod data_store;
mod menu;

use menu::Menu;
use data_store::{ 
	data_router::DataRouterState, data_router::DataRouter,
	error_router::ErrorRouter};
use databases::{PasswordDatabase, UserDatabase, UserLookup, AlarmDatabase};

use std::sync::atomic::{AtomicUsize};
use std::sync::{Arc, RwLock};
use std::collections::HashMap;

use actix::prelude::*;
use threadpool::ThreadPool;
use structopt::StructOpt;

/// A basic example
#[derive(StructOpt)]
#[structopt(name = "dataserver")]
struct Opt {
    #[structopt(short, long)]
	create_new_certificate: bool,
	#[structopt(short, long)]
	no_menu: bool,

	#[cfg(feature = "stable")]
    #[structopt(short = "p", long = "port", default_value = "443")]
	port: u16,
	
	#[cfg(not(feature = "stable"))]
    #[structopt(short = "p", long = "port", default_value = "8443")]
	port: u16,
	
    #[structopt(short = "t", long = "token")]
	token: String,
	
    #[structopt(short = "d", long = "domain")]
    domain: String,
}

#[actix_rt::main]
async fn main() {
	let opt = Opt::from_args();
	
	//only do if certs need update
	if opt.create_new_certificate {
		//generate_and_sign_keys
		if let Err(error) = certificate_manager::generate_and_sign_keys(
			&opt.domain, "keys/cert.key", "keys/cert.cert", "keys/user.key",
		).await {
			println!("could not auto generate certificate, error: {:?}", error)
		}
	}

	error::setup_logging(1).expect("could not set up debugging");
	let db = sled::Config::default() //651ms
			.path("database")
			.flush_every_ms(None) //do not flush to disk unless explicitly asked
			.cache_capacity(1024 * 1024 * 32) //32 mb cache 
			.open().unwrap();

	//TODO can a tree be opened multiple times?
	let passw_db = PasswordDatabase::from_db(&db).unwrap();
	let user_db = UserDatabase::from_db(&db).unwrap();
	let alarm_db = AlarmDatabase::from_db(&db).unwrap();
	let db_lookup = UserLookup::from_user_db(&user_db).unwrap();
	
	let data = Arc::new(RwLock::new(data_store::init("data").unwrap()));
	
	let sessions = Arc::new(RwLock::new(HashMap::new()));
	let bot_pool = ThreadPool::new(2);
	
	let data_router_addr = DataRouter::new(&data, alarm_db.clone(), 
		opt.token.clone()).start();
	
	let error_router_addr = ErrorRouter::load(&db, data.clone())
	.unwrap().start();

    let data_router_state = DataRouterState {
        passw_db: passw_db.clone(),
		user_db: user_db.clone(),
		alarm_db: alarm_db.clone(),
		db_lookup: db_lookup.clone(),
		bot_pool,
		bot_token: opt.token.clone(),

        data_router_addr: data_router_addr.clone(),
        error_router_addr: error_router_addr.clone(),
        data: data.clone(),
        sessions: sessions.clone(),
        free_session_ids: Arc::new(AtomicUsize::new(0)),
        free_ws_session_ids: Arc::new(AtomicUsize::new(0)),
    };

	//runs in its own thread
	let web_handle = httpserver::start(
        "keys/cert.key", 
        "keys/cert.cert", 
        "keys/intermediate.cert", 
		data_router_state.clone(),
		opt.port,
		opt.domain.clone(),
	);
    bot::set_webhook(&opt.domain, &opt.token, opt.port).await.unwrap();
	
	let menu_future = if !opt.no_menu {
		Menu::gui(data, passw_db, user_db, alarm_db, db_lookup)
	} else {
		Menu::simple()
	};

	menu_future.await;
	web_handle.stop(false).await;
}