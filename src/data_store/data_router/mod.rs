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
use super::{Data, Field};

use crate::databases::{PasswordDatabase, WebUserDatabase, BotUserDatabase};
use crate::httpserver::Session;

mod alarms;
pub use alarms::{Alarm, CompiledAlarm, NotifyVia};
//pub use alarms::{AddAlarm};

#[derive(Clone)]
pub struct DataRouterState {
	pub passw_db: PasswordDatabase,
	pub web_user_db: WebUserDatabase,
	pub bot_user_db: BotUserDatabase,
	pub bot_pool: ThreadPool,

	pub data_router_addr: Addr<DataRouter>,
	pub error_router_addr: Addr<error_router::ErrorRouter>,

	pub data: Arc<RwLock<Data>>,

	pub sessions: Arc<RwLock<HashMap<u16, Arc<Mutex<Session>> >>> ,
	pub free_session_ids: Arc<AtomicUsize>,
	pub free_ws_session_ids: Arc<AtomicUsize>,
}

type UserName = String;
type ClientSessionId = u16;
pub struct DataRouter {
	sessions: HashMap<ClientSessionId, Clientinfo>,
	subs: HashMap<DatasetId, HashSet<ClientSessionId>>,
	meta: HashMap<DatasetId, Vec<Field<f32>>>,
	alarms_by_set: HashMap<DatasetId, HashMap<alarms::Id, (CompiledAlarm, UserName)>>,
	alarms_by_username: HashMap<UserName, Vec<(DatasetId, alarms::Id, Alarm)>>,
	alarm_context: HashMapContext,
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

	pub fn new(data: &Arc<RwLock<Data>>) -> DataRouter {
		let meta = data.read().unwrap().sets
			.iter()
			.map(|(id,set)| (*id, set.metadata.fields.clone() ))
			.collect();

		DataRouter {
			sessions: HashMap::new(),
			subs: HashMap::new(),
			meta,//TODO load the next two from db
			alarms_by_set: HashMap::new(),
			alarms_by_username: HashMap::new(),
			alarm_context: HashMapContext::new(),
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
		dbg!();
		println!("test");
		let updated_dataset_id = msg.from_id;

		//check all alarms that could go off
		if self.alarms_by_set.contains_key(&updated_dataset_id){
			let now = Utc::now();
			self.update_context(&msg.line, &updated_dataset_id); //Opt: 
			if let Some(alarms) = self.alarms_by_set.get(&updated_dataset_id){
				for (alarm, _) in alarms.values() {
					alarm.evalute(&mut self.alarm_context, &now).unwrap();
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

#[derive(Message, Clone)]
#[rtype(result = "()")]
pub struct NewData2 {
	pub from_id: DatasetId,
	pub line: Vec<u8>,
	pub timestamp: i64,
}

impl Handler<NewData2> for DataRouter {
	type Result = ();

	fn handle(&mut self, msg: NewData2, _: &mut Context<Self>) -> Self::Result {
		dbg!();
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

//#[derive(Message, Clone)]
pub struct AddAlarm {
    pub alarm: Alarm,
    pub username: String,
    pub sets: Vec<DatasetId>,
}

impl Message for AddAlarm {
	type Result = usize;
}

impl Handler<AddAlarm> for DataRouter {
	//type Result = Result<(),AlarmError>;
	type Result = usize;

	fn handle(&mut self, msg: AddAlarm, _: &mut Context<Self>) -> Self::Result {
		dbg!("add alarm?");
		let mut set_id_alarm = Vec::with_capacity(msg.sets.len()); 
        for set_id in msg.sets {
			dbg!(&set_id);
			let list = self.alarms_by_set.get_mut(&set_id).unwrap();
            
            let free_id = (std::u8::MIN..std::u8::MAX)
                .skip_while(|x| list.contains_key(x))
                .next().unwrap();//.ok_or(AlarmError::TooManyAlarms).unwrap();//?;
			
			let alarm: CompiledAlarm = msg.alarm.clone().into();
			list.insert(free_id, (alarm, msg.username.clone())).unwrap();
            set_id_alarm.push((set_id, free_id, msg.alarm.clone()));
        }
		self.alarms_by_username.insert(msg.username, set_id_alarm).unwrap();
		
		10usize
		//Ok(())
	}
}

pub struct Clientinfo {
	addr: Recipient<NewData>,
	subs: Vec<DatasetId>,
}

/// New chat session is created
#[derive(Message)]
#[rtype(u16)]
pub struct DebugActix {
	pub test_numb: u16,
}

impl Handler<DebugActix> for DataRouter {
	type Result = u16;

	fn handle(&mut self, msg: DebugActix, _: &mut Context<Self>) -> Self::Result {
		dbg!();
		let numb = msg.test_numb;
		let numb = numb + 1;
		numb
	}
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

