use std::sync::{Arc, RwLock, Mutex};
use std::sync::atomic::{AtomicUsize};

use log::{debug, trace};
use actix::prelude::*;

use std::collections::{HashMap, HashSet};

use super::DatasetId;
use super::error_router;
use super::Data;

use crate::databases::{PasswordDatabase, WebUserDatabase, BotUserDatabase};
use crate::httpserver::Session;

#[derive(Clone)]
pub struct DataRouterState {
	pub passw_db: PasswordDatabase,
	pub web_user_db: WebUserDatabase,
	pub bot_user_db: BotUserDatabase,

	pub data_router_addr: Addr<DataRouter>,
	pub error_router_addr: Addr<error_router::ErrorRouter>,

	pub data: Arc<RwLock<Data>>,

	pub sessions: Arc<RwLock<HashMap<u16, Arc<Mutex<Session>> >>> ,
	pub free_session_ids: Arc<AtomicUsize>,
	pub free_ws_session_ids: Arc<AtomicUsize>,
}

//TODO extract alarms to theire own module and finish
// -add database tree for storing data_alarms
// -add handler for setting and removing data_alarms
// -add function for httpserver to list data_alarms

enum SimpleAlarmVariant {
	Over,
	Under,
}

struct NotifyMethod {
	email: Option<String>,
	telegram: Option<()>,
	custom: bool,
}

struct SimpleAlarm {
	field_id: u16,
	threshold_value: f32,
	variant: SimpleAlarmVariant,
	nofify: NotifyMethod,
	message: String,
}

impl SimpleAlarm {
	fn evalute(&self, msg: &NewData) {
		unimplemented!();
	}
}

type ClientSessionId = u16;
pub struct DataRouter {
	sessions: HashMap<ClientSessionId, Clientinfo>,
	subs: HashMap<DatasetId, HashSet<ClientSessionId>>,
	alarms : HashMap<DatasetId, HashSet<SimpleAlarm>>,
}


#[derive(Message, Clone)]
pub struct NewData {
	pub from_id: DatasetId,
	pub line: Vec<u8>,
	pub timestamp: i64,
}

impl Handler<NewData> for DataRouter {
	type Result = ();

	fn handle(&mut self, msg: NewData, _: &mut Context<Self>) -> Self::Result {
		//debug!("NewData, subs: {:?}", self.subs);
		let updated_dataset_id = msg.from_id;
		//get a list of clients connected to the datasource with new data
		
		if let Some(alarms) = self.alarms.get(&updated_dataset_id){
			for alarm in alarms {
				alarm.evalute(&msg);
			}
		}
		
		if let Some(subs) = self.subs.get(&updated_dataset_id){
			debug!("subs: {:?}", subs);
			for websocket_session_id in subs.iter() {
				//println!("sending signal");
				// foward new data message to actor that maintains the
				// websocket connection with this client.
				let client_websocket_handler = &self.sessions.get(websocket_session_id).unwrap().addr;
				client_websocket_handler.do_send(msg.clone()).unwrap();
			}
		}
	}
}

/// New chat session is created
#[derive(Message)]
#[rtype(u16)]
pub struct Connect {
	pub addr: Recipient<NewData>,
	pub ws_session_id: u16,
}

impl Handler<Connect> for DataRouter {
	type Result = u16;

	fn handle(&mut self, msg: Connect, _: &mut Context<Self>) -> Self::Result {
		// register session with random id
		let id = msg.ws_session_id;
		self.sessions.insert(
			id,
			Clientinfo {
				addr: msg.addr,
				subs: Vec::new(),
			},
		);

		// send id back
		id
	}
}

#[derive(Message)]
pub struct Disconnect {
	pub ws_session_id: u16,
}

/// Handler for Disconnect message.
impl Handler<Disconnect> for DataRouter {
	type Result = ();

	fn handle(&mut self, msg: Disconnect, _: &mut Context<Self>) {
		// remove address
		if let Some(client_info) = self.sessions.remove(&msg.ws_session_id) {
			for sub in client_info.subs {
				if let Some(subbed_clients) = self.subs.get_mut(&sub) {
					subbed_clients.remove(&msg.ws_session_id);
					trace!("removed client from: sub:{:?} ", sub);
				}
			}
		}
	}
}

/// New chat session is created
#[derive(Message)]
pub struct SubscribeToSource {
	pub ws_session_id: u16,
	pub set_id: DatasetId,
}

impl Handler<SubscribeToSource> for DataRouter {
	type Result = ();

	fn handle(&mut self, msg: SubscribeToSource, _: &mut Context<Self>) -> Self::Result {
		let SubscribeToSource { ws_session_id, set_id } = msg;
		let client_info = self.sessions.get_mut(&ws_session_id).unwrap();
		client_info.subs.push(set_id);

		trace!("subscribing to source: {:?}",set_id);
		//fix when non lexical borrow checker arrives
		if let Some(subscribers) = self.subs.get_mut(&set_id) {
			subscribers.insert(ws_session_id);
			//println!("added new id to subs");
			return ();
		}

		let mut subscribers = HashSet::new();
		subscribers.insert(ws_session_id);
		self.subs.insert(set_id, subscribers);
		()
	}
}

pub struct Clientinfo {
	addr: Recipient<NewData>,
	subs: Vec<DatasetId>,
}

impl Default for DataRouter {
	fn default() -> DataRouter {
		let subs = HashMap::new();

		DataRouter {
			sessions: HashMap::new(),
			subs: subs,
			alarms: HashMap::new(),
		}
	}
}

/// Make actor from `ChatServer`
impl Actor for DataRouter {
	/// We are going to use simple Context, we just need ability to communicate
	/// with other actors.
	type Context = Context<Self>;
}

///////////////////////////////////
