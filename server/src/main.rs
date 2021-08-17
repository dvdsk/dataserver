//#[cfg(test)] //TODO: adapt tests and re-enable
//mod test;
mod bot;
mod data_store;
mod database;
mod debug_middleware;
mod error;
mod httpserver;
mod admin_interface;

use data_store::{
	data_router::DataRouter, data_router::DataRouterState, error_router::ErrorRouter,
};
use database::{AlarmDatabase, PasswordDatabase, UserDatabase, UserLookup};

use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, RwLock};

use actix::prelude::*;
use log::error;
use structopt::StructOpt;

/// A basic example
#[derive(StructOpt)]
#[structopt(name = "dataserver")]
struct Opt {
	#[structopt(short, long)]
	no_menu: bool,

	#[structopt(short, long)]
	service: bool,

	/// Port for incomming trafic
	#[structopt(short = "p", long = "port")]
	port: u16,

	/// Advertise this port to telegram for websocket
	#[structopt(short = "e", long = "external-port", default_value="443")]
	external_port: u16,

    // Telegram bot token
	#[structopt(short = "t", long = "token")]
	token: String,

	/// domain, for the webserver www will be added automatically
	#[structopt(short = "d", long = "domain")]
	domain: String,

	/// upgrade the database from a previous sled version
	#[structopt(short = "u", long = "upgrade-db")]
	upgrade_db: bool,
}

#[actix_rt::main]
async fn main() {
	let opt = Opt::from_args();

	error::setup_logging(3).expect("could not set up debugging");
	let db = if opt.upgrade_db {
		log::warn!("upgrading database! .....");
		std::fs::rename("database", "database_old").unwrap();
		let old = old_sled::open("database_old").unwrap();
		let export = old.export();
		let db = sled::open("database").unwrap();
		db.import(export);
		log::warn!("done upgrading database");
		db
	} else {
		sled::Config::default() //651ms
			.path("database")
			.flush_every_ms(None) //do not flush to disk unless explicitly asked
			.cache_capacity(1024 * 1024 * 32) //32 mb cache
			.open()
			.unwrap()
	};

	let passw_db = PasswordDatabase::from_db(&db).unwrap();
	let user_db = UserDatabase::from_db(&db).unwrap();
	let alarm_db = AlarmDatabase::from_db(&db).unwrap();
	let db_lookup = UserLookup::from_user_db(&user_db).unwrap();

	let data = Arc::new(RwLock::new(data_store::init("data").unwrap()));

	let sessions = Arc::new(RwLock::new(HashMap::new()));

	let data_router_addr = DataRouter::new(&data, alarm_db.clone(), opt.token.clone()).start();

	let error_router_addr = ErrorRouter::load(&db, data.clone()).unwrap().start();

	let data_router_state = DataRouterState {
		passw_db: passw_db.clone(),
		user_db: user_db.clone(),
		alarm_db: alarm_db.clone(),
		db_lookup: db_lookup.clone(),
		bot_token: opt.token.clone(),

		data_router_addr: data_router_addr.clone(),
		error_router_addr: error_router_addr.clone(),
		data: data.clone(),
		sessions: sessions.clone(),
		free_session_ids: Arc::new(AtomicUsize::new(0)),
		free_ws_session_ids: Arc::new(AtomicUsize::new(0)),
	};

	//runs in its own thread
	let http_server = httpserver::start( // TODO get out of seperate thread into event loop
		data_router_state.clone(),
		opt.port,
		opt.domain.clone(),
	);

    let res = bot::set_webhook(&opt.domain, &opt.token, opt.external_port).await;
	if let Err(e) = res {
		error!("could not start telegram bot: {:?}", e);
	}

    http_server.await;
    // TODO add menu interface, it will block forever
}
