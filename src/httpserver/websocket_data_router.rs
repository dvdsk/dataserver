extern crate actix;
extern crate actix_web;
extern crate rand;
use self::actix::prelude::*;

use self::rand::{Rng, ThreadRng};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use timeseries_interface::{DatasetId};

///// New chat session is created
//#[derive(Message)]
//#[rtype(usize)]
//pub struct Connect {
//pub addr: Recipient<Message>,
//}

#[derive(Message)]
pub struct ClientMessage(pub String);

#[derive(Message)]
pub struct NewData {
	pub from: DatasetId,
}

impl Handler<NewData> for DataServer {
	type Result = ();

	fn handle(&mut self, msg: NewData, _: &mut Context<Self>) -> Self::Result {
		println!("NewData, subs: {:?}", self.subs);
		println!("there is new data");
		let updated_dataset_id = msg.from;
		//get a list of clients connected to the datasource with new data
		if let Some(subs) = self.subs.get(&updated_dataset_id){
			for client_session_id in subs.iter() {
				// foward new data message to actor that maintains the
				// websocket connection with this client.
				let client_websocket_handler = &self.sessions.get(client_session_id).unwrap().addr;
				client_websocket_handler.do_send(NewData{from: updated_dataset_id});//FIXME //TODO
			}
		}
	}
}

/// New chat session is created
#[derive(Message)]
#[rtype(u16)]
pub struct Connect {
	pub addr: Recipient<NewData>,
	pub session_id: u16,
}

impl Handler<Connect> for DataServer {
	type Result = u16;

	fn handle(&mut self, msg: Connect, _: &mut Context<Self>) -> Self::Result {
		println!("Someone joined");
		// register session with random id
		let id = msg.session_id;
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
	pub session_id: u16,
}

/// Handler for Disconnect message.
impl Handler<Disconnect> for DataServer {
	type Result = ();

	fn handle(&mut self, msg: Disconnect, _: &mut Context<Self>) {
		println!("Someone disconnected");
		// remove address
		if let Some(client_info) = self.sessions.remove(&msg.session_id) {
			for sub in client_info.subs {
				if let Some(subbed_clients) = self.subs.get_mut(&sub) {
					subbed_clients.remove(&msg.session_id);
					println!("removed client from: sub:{:?} ", sub);
				}
			}
		}
	}
}

/// New chat session is created
#[derive(Message)]
pub struct SubscribeToSource {
	pub session_id: u16,
	pub set_id: DatasetId,
}

impl Handler<SubscribeToSource> for DataServer {
	type Result = ();

	fn handle(&mut self, msg: SubscribeToSource, _: &mut Context<Self>) -> Self::Result {
		let SubscribeToSource { session_id, set_id } = msg;
		let client_info = self.sessions.get_mut(&session_id).unwrap();
		client_info.subs.push(set_id);

		//fix when non lexical borrow checker arrives
		if let Some(subscribers) = self.subs.get_mut(&set_id) {
			subscribers.insert(session_id);
			println!("added new id to subs");
			return ();
		}

		let mut subscribers = HashSet::new();
		subscribers.insert(session_id);
		self.subs.insert(set_id, subscribers);
		()
	}
}

pub struct Clientinfo {
	addr: Recipient<NewData>,
	subs: Vec<DatasetId>,
}

pub struct DataServer {
	sessions: HashMap<u16, Clientinfo>,
	subs: HashMap<DatasetId, HashSet<u16>>,

	rng: RefCell<ThreadRng>,
}

impl Default for DataServer {
	fn default() -> DataServer {
		let mut subs = HashMap::new();
		subs.insert(0, HashSet::new());
		subs.insert(1, HashSet::new());
		subs.insert(2, HashSet::new());
		subs.insert(3, HashSet::new());

		DataServer {
			sessions: HashMap::new(),
			subs: subs,

			rng: RefCell::new(rand::thread_rng()),
		}
	}
}

/// Make actor from `ChatServer`
impl Actor for DataServer {
	/// We are going to use simple Context, we just need ability to communicate
	/// with other actors.
	type Context = Context<Self>;
}

///////////////////////////////////
