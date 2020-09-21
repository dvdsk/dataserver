//#[cfg(test)] //TODO: adapt tests and re-enable
//mod test;
mod bot;
mod data_store;
mod databases;
mod debug_middleware;
mod error;
mod httpserver;
mod menu;

use data_store::{
	data_router::DataRouter, data_router::DataRouterState, error_router::ErrorRouter,
};
use databases::{AlarmDatabase, PasswordDatabase, UserDatabase, UserLookup};
use menu::Menu;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;

use actix::prelude::*;
use log::error;
use structopt::StructOpt;
use threadpool::ThreadPool;

/// A basic example
#[derive(StructOpt)]
#[structopt(name = "dataserver")]
struct Opt {
	#[structopt(short, long)]
	create_new_certificate: bool,

	#[structopt(short, long)]
	no_menu: bool,

	#[structopt(short, long)]
	service: bool,

	/// Port for incomming trafic
	#[cfg(feature = "stable")]
	#[structopt(short = "p", long = "port", default_value = "443")]
	port: u16,

	#[cfg(not(feature = "stable"))]
	#[structopt(short = "p", long = "port", default_value = "8443")]
	port: u16,

	/// Advertise this port to clients. By default the port
	/// for clients is the same as is listend on
	#[structopt(short = "e", long = "external-port")]
	external_port: Option<u16>,

	#[structopt(short = "t", long = "token")]
	token: String,

	/// domain without used subdomain fox example "example.com".
	/// a variant with www attached will be added automatically
	#[structopt(short = "d", long = "domain")]
	domain: String,

	/// directory to look in for certificate and pirvate key
	#[structopt(short = "k", long = "keys", default_value = "keys")]
	key_dir: PathBuf,

	/// upgrade the database from a previous sled version
	#[structopt(short = "u", long = "upgrade-db")]
	upgrade_db: bool,
}

#[actix_rt::main]
async fn main() {
	let opt = Opt::from_args();

	//only do if certs need update
	if opt.create_new_certificate {
		//generate_and_sign_keys
		if let Err(error) = cert_manager::generate_and_sign_keys_guided(
			"dataserver",
			&opt.domain,
			&opt.key_dir,
			true,
		) {
			//TODO change to false
			error!("could not auto generate certificate, error: {:?}", error)
		}
	}

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
	let bot_pool = ThreadPool::new(2);

	let data_router_addr = DataRouter::new(&data, alarm_db.clone(), opt.token.clone()).start();

	let error_router_addr = ErrorRouter::load(&db, data.clone()).unwrap().start();

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
		data_router_state.clone(),
		&opt.key_dir,
		opt.port,
		opt.domain.clone(),
	)
	.unwrap();

	let res = if let Some(port) = opt.external_port {
		bot::set_webhook(&opt.domain, &opt.token, port).await
	} else {
		bot::set_webhook(&opt.domain, &opt.token, opt.port).await
	};
	if let Err(e) = res {
		error!("could not start telegram bot: {:?}", e);
	}

	if opt.service {
		loop {
			thread::sleep(Duration::from_secs(60 * 60 * 24));
		} //TODO replace with something nice
	} else {
		let menu_future = if !opt.no_menu {
			Menu::gui(data, passw_db, user_db, alarm_db, db_lookup)
		} else {
			Menu::simple()
		};
		menu_future.await;
	}

	web_handle.stop(false).await;
}
