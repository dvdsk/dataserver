//#[cfg(test)] //TODO: adapt tests and re-enable
//mod test;

mod certificate_manager;
mod config;
mod debug_middleware;
mod error;
mod bot;
mod databases;
mod httpserver;
mod data_store;
mod menu;

use menu::Menu;
use actix::prelude::*;
use threadpool::ThreadPool;
use std::sync::atomic::{AtomicUsize};
use data_store::{error_router, data_router, data_router::DataRouterState};

use databases::{PasswordDatabase, WebUserDatabase, BotUserDatabase};

use std::sync::{Arc, RwLock};
use std::collections::HashMap;

use structopt::StructOpt;

/// A basic example
#[derive(StructOpt)]
#[structopt(name = "dataserver")]
struct Opt {
    #[structopt(short, long)]
	create_new_certificate: bool,
	#[structopt(short, long)]
	no_menu: bool,
}

#[actix_rt::main]
async fn main() {
	let opt = Opt::from_args();
	
	//only do if certs need update
	if opt.create_new_certificate {
		//generate_and_sign_keys
		if let Err(error) = certificate_manager::generate_and_sign_keys(
			config::DOMAIN, "keys/cert.key", "keys/cert.cert", "keys/user.key",
		).await {
			println!("could not auto generate certificate, error: {:?}", error)
		}
	}

	error::setup_logging(1).expect("could not set up debugging");
	let config = sled::ConfigBuilder::new() //651ms
			.path("database".to_owned())
			.flush_every_ms(None) //do not flush to disk unless explicitly asked
			.async_io(true)
			.cache_capacity(1024 * 1024 * 32) //32 mb cache 
			.build();
	let db = sled::Db::start(config).unwrap();

	//TODO can a tree be opened multiple times?
	let passw_db = PasswordDatabase::from_db(&db).unwrap();
	let web_user_db = WebUserDatabase::from_db(&db).unwrap();
	let bot_user_db = BotUserDatabase::from_db(&db).unwrap();
	
	let data = Arc::new(RwLock::new(data_store::init("data").unwrap()));
	
	let sessions = Arc::new(RwLock::new(HashMap::new()));
	let bot_pool = ThreadPool::new(2);
	
    let data_router_addr = data_router::DataRouter::new(&data).start();
	
	let error_router_addr = error_router::ErrorRouter::load(&db, data.clone())
	.unwrap().start();

    let data_router_state = DataRouterState {
        passw_db: passw_db.clone(),
        web_user_db: web_user_db.clone(),
		bot_user_db: bot_user_db.clone(),
		bot_pool,
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
	);
    bot::set_webhook(config::DOMAIN, config::TOKEN, config::PORT).await.unwrap();
	
	let menu_future = if !opt.no_menu {
		Menu::gui(data, passw_db, web_user_db, bot_user_db)
	} else {
		Menu::simple()
	};

	menu_future.await;
	web_handle.stop(false).await;
}