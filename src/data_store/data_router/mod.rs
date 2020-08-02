use std::sync::{Arc, RwLock, Mutex};
use std::sync::atomic::{AtomicUsize};

use chrono::Utc;
use log::{debug, trace};
use actix::prelude::*;
use threadpool::ThreadPool;
use evalexpr::{HashMapContext, Context as evalContext};

use std::collections::{HashMap, HashSet};

use super::DatasetId;
use super::error_router;
use super::{Data, MetaField};

use crate::httpserver::Session;
use crate::databases::{PasswordDatabase, UserDatabase, 
	UserLookup, 
	AlarmDatabase,
	UserId, AlarmId};

mod alarms;
pub use alarms::{Alarm, CompiledAlarm, NotifyVia, AddAlarm, RemoveAlarm};

#[derive(Clone)]
pub struct DataRouterState {
	pub passw_db: PasswordDatabase,
	pub user_db: UserDatabase,
	pub alarm_db: AlarmDatabase,
	pub db_lookup: UserLookup,
	pub bot_pool: ThreadPool,
	pub bot_token: String,

	pub data_router_addr: Addr<DataRouter>,
	pub error_router_addr: Addr<error_router::ErrorRouter>,

	pub data: Arc<RwLock<Data>>,

	pub sessions: Arc<RwLock<HashMap<u16, Arc<Mutex<Session>> >>> ,
	pub free_session_ids: Arc<AtomicUsize>,
	pub free_ws_session_ids: Arc<AtomicUsize>,
}

type ClientSessionId = u16;
pub struct DataRouter {
	sessions: HashMap<ClientSessionId, Clientinfo>,
	subs: HashMap<DatasetId, HashSet<ClientSessionId>>,
	meta: HashMap<DatasetId, Vec<MetaField<f32>>>,
	alarms_by_set: HashMap<DatasetId, HashMap<(UserId,AlarmId), CompiledAlarm>>,
	alarm_context: HashMapContext,
	async_pool: ThreadPool,
	bot_token: String,
}

impl DataRouter {
	fn update_context(&mut self, line: &Vec<u8>, set_id: &DatasetId) {
		let fields = self.meta.get(set_id).unwrap();
		for field in fields {
			let value: f64 = field.decode(&line);
			let name = format!("{}_{}",set_id,field.id);
			self.alarm_context.set_value(name.into(),value.into()).unwrap();
		}
	}

	//TODO get full alarm Id from iter method
	//finish insertion
	pub fn new(data: &Arc<RwLock<Data>>, alarm_db: AlarmDatabase, 
		bot_token: String) -> DataRouter {
		
		type AlarmList = HashMap<(UserId,AlarmId), CompiledAlarm>;
		
		//collect metadata on all datasets
		let meta = data.read().unwrap().sets.iter()
			.map(|(id,set)| (*id, set.metadata.fields.clone() ))
			.collect();
		
		//read alarms from the database into lookup hashmap
		let mut alarms_by_set: HashMap<DatasetId, AlarmList> = HashMap::new();
		for (owner_id, alarm_id, alarm) in alarm_db.iter(){
			for set in alarm.watched_sets() {
				let compiled_alarm = CompiledAlarm::from(alarm.clone());
				if let Some(list) = alarms_by_set.get_mut(&set) {
					list.insert((owner_id, alarm_id), compiled_alarm);
				} else {
					let mut list: AlarmList = HashMap::new();
					list.insert((owner_id, alarm_id), compiled_alarm);
					alarms_by_set.insert(set, list);
				}
			}
		}

		DataRouter {
			sessions: HashMap::new(),
			subs: HashMap::new(),
			bot_token, 
			meta,
			alarms_by_set,
			alarm_context: HashMapContext::new(),
			async_pool: ThreadPool::new(2),
		}
	}
}

#[derive(Message, Clone)]
#[rtype(result = "()")]
pub struct NewData {
	pub from_id: DatasetId,
	pub line: Vec<u8>,
	pub timestamp: i64,
}

impl Handler<NewData> for DataRouter {
	type Result = ();

	fn handle(&mut self, msg: NewData, _: &mut Context<Self>) -> Self::Result {
		let updated_dataset_id = msg.from_id;

		//check all alarms that could go off
		if self.alarms_by_set.contains_key(&updated_dataset_id){
			let now = Utc::now();
			self.update_context(&msg.line, &updated_dataset_id); //Opt: 
			if let Some(alarms) = self.alarms_by_set.get_mut(&updated_dataset_id){
				for alarm in alarms.values_mut() {
					let token = 
					alarm.evalute(&mut self.alarm_context, 
						&now, &self.async_pool, self.bot_token.clone());
				}
			}
		}

		//get a list of clients connected to the datasource with new data
		if let Some(subs) = self.subs.get(&updated_dataset_id){
			debug!("subs: {:?}", subs);
			for websocket_session_id in subs.iter() {
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
#[rtype(result = "()")]
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
#[rtype(result = "()")]
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

/// Make actor
impl Actor for DataRouter {
	/// We are going to use simple Context, we just need ability to communicate
	/// with other actors.
	type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Context<Self>) {
        // start heartbeats otherwise server will disconnect after 10 seconds
        dbg!("started datarouter");
    }
}

///////////////////////////////////

