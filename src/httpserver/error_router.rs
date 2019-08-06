use log::{debug, trace};
use actix::prelude::*;
use std::sync::{Arc, RwLock};

use bincode;
use serde::{Serialize, Deserialize};
use chrono::{DateTime, offset::Utc};
use std::collections::{HashMap, HashSet};

use crate::httpserver::timeseries_interface::{Data, DatasetId, FieldId};
use crate::error::DataserverError;

/*
	Errors for sensors and custom system
		
	msg:
	---------------------------------------------------------
	-- dataset_id [u16]-- field_id [u16] -- error code [u8]--
	---------------------------------------------------------

	dataset_id zero (0) is reserved for custom errors that should be reported to the web server.
	field_id zero (0) is reserved for errors not relevant to a field (=sensor) but the entire dataset 
	the first 124 error codes are generic errors, from 125 and higher are specific to the sensor
*/

pub type ErrorCode = u8;

struct ReportedErrors {
	tree: Arc<sled::Tree>,
}

impl ReportedErrors {
	fn load(db: &sled::Db) -> Result<Self, DataserverError> {
		Ok(Self{ tree:db.open_tree("reported_errors")?})
	}

	//return true if this error was reported within a day, if it was not remembers the 
	//error as reported now
	fn recently_reported(&mut self, msg: &NewError) -> Result<bool,DataserverError> {
		//errors are stored based on 32bit key these are sorted as:
		//-----3-------2--------------1-----------------0---------- (byte)
		//-- dataset_id [u16]-- field_id [u16] -- error code [u8]--
		//---------------------------------------------------------
		//dataset_id in big endian representation, this allows us
		//to use ranges in database querys

		let mut key: [u8; 4] = [0u8;4];
		key[2..4].copy_from_slice(&msg.dataset_id.to_be_bytes());
		key[1] = msg.field_id;
		key[0] = msg.error_code;

		if let Some(last_reported) = self.tree.get(&key).unwrap(){
			let last_reported: DateTime<Utc> = bincode::deserialize(&last_reported)?;
			if last_reported.signed_duration_since(Utc::now()) > chrono::Duration::days(1) {
				self.tree.set(&key, bincode::serialize(&Utc::now())?)?;
				Ok(true)
			} else {
				Ok(false)
			}
		} else {
			self.tree.set(&key, bincode::serialize(&Utc::now())?)?;
			Ok(false)
		}
	}
}

struct NotifyChannels {
	tree: Arc<sled::Tree>,
}

#[derive(Serialize, Deserialize)]
struct NotifyOptions {
	email: Option<String>,
	telegram: Option<()>,
}

impl NotifyChannels {
	fn load(db: &sled::Db) -> Result<Self, DataserverError> {
		Ok(Self{ tree:db.open_tree("notify_channels")?})
	}

	//return true if this error was reported within a day, if it was not remembers the 
	//error as reported now
	fn should_notify(&mut self, msg: &NewError) 
	-> Result<Option<Vec<NotifyOptions>>, DataserverError>{
		//errors are stored based on 32bit key these are sorted as:
		//-----3-------2--------------1-----------------0---------- (byte)
		//-- dataset_id [u16]-- field_id [u16] -- error code [u8]--
		//---------------------------------------------------------
		//dataset_id in big endian representation, this allows us
		//to use ranges in database querys

		let mut key: [u8; 3] = [0u8;3];
		key[1..3].copy_from_slice(&msg.dataset_id.to_be_bytes());
		key[1] = msg.field_id;
		
		if let Some(to_notify) = self.tree.get(&key)?{
			let to_notify: Vec<NotifyOptions> = bincode::deserialize(&to_notify)?;
			Ok(Some(to_notify))
		} else {
			Ok(None)
		}
	}
}

type ClientSessionId = u16;
//Errors are grouped by dataset
//TODO add general error log
pub struct ErrorRouter {
	sessions: HashMap<ClientSessionId, Clientinfo>,
	subs: HashMap<DatasetId, HashSet<ClientSessionId>>,
	
 	//TODO speed this up dramatically by using an in memory representation for reads and updating Db on write
	clients_to_notify: NotifyChannels, //keys = dataset_id+field_id
	client_undisplayed_errors: Arc<sled::Tree>, // display as soon as client loads/connects
	reported_errors: ReportedErrors,

	data: Arc<RwLock<Data>>,
}


#[derive(Message, Clone)]
pub struct NewError {
	pub dataset_id: DatasetId,
	pub field_id: FieldId,
	pub error_code: ErrorCode,
	pub timestamp: DateTime<Utc>,
}

///Formats error codes:
/// if field_id and dataset_id > 0
/// 	[time] data collection error in set: [dataset name]([dataset description]) specificly [field_id name] reports: [error code explanatin]
/// if the field_id == 0
/// 	[time] data collection error in set: [dataset name]([dataset description]) error message: [error code explanatin]
/// if the dataset_id == 0 then this is a system error and is reported on as follows:
/// 	[time] system error [error code explanation]

fn format_error_code(data: &Arc<RwLock<Data>>, msg: &NewError) -> Result<String, ()> {
	//TODO add timestamp
	if msg.dataset_id == 0 {
		return Ok(format!("{time} system error occured: {error}", 
			time=msg.timestamp, 
			error=msg.error_code
			).to_string());
	} 
	
	if let Some(dataset) = data.read().unwrap().sets.get(&msg.dataset_id) {
		let metadata = &dataset.metadata;
		if msg.field_id == 0 {
			Ok(format!("{time} error during data collection, {dataset_name}({dataset_description}) reports: {error}",
				time=msg.timestamp, 
				dataset_name=metadata.name,
				dataset_description = metadata.description,
				error=msg.error_code,
				).to_string())
		} else if let Some(field) = metadata.fields.get(msg.field_id as usize) {
			Ok(format!("{time} error during data collection, {field_name} in {dataset_name}({dataset_description}) reports: {error}",
				time=msg.timestamp, 
				field_name=field.name,
				dataset_name=metadata.name,
				dataset_description = metadata.description,
				error=msg.error_code,
				).to_string())
		} else {
			Err(())
		}
	} else {
		Err(())
	}
} 

impl Handler<NewError> for ErrorRouter {
	type Result = ();

	fn handle(&mut self, msg: NewError, _: &mut Context<Self>) -> Self::Result {
		if self.reported_errors.recently_reported(&msg).unwrap(){ return; }
		
		let error_msg = format_error_code(&self.data, &msg);
		
		//get a list of clients connected interested in this dataset
		if let Some(subs) = self.subs.get(&msg.dataset_id){
			debug!("subs: {:?}", subs);
			for client_session_id in subs.iter() {
				// foward new data message to actor that maintains the
				// websocket connection with this client.
				let client_websocket_handler = &self.sessions.get(client_session_id).unwrap().addr;
				client_websocket_handler.do_send(msg.clone()).unwrap();
			}
		}
		//fetch the list of notification channels from
		if let Some(to_notify) = self.clients_to_notify.should_notify(&msg).unwrap(){
			for notify_option in to_notify{
				if let Some(mail_adress) = notify_option.email {
					unimplemented!();
				} 
				if let Some(telegram) = notify_option.telegram {
					unimplemented!();
				}
			}
		}
	}
}

/// New chat session is created
#[derive(Message)]
#[rtype(u16)]
pub struct Connect {
	pub addr: Recipient<NewError>,
	pub ws_session_id: u16,
}

impl Handler<Connect> for ErrorRouter {
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
impl Handler<Disconnect> for ErrorRouter {
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

impl Handler<SubscribeToSource> for ErrorRouter {
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
	addr: Recipient<NewError>,
	subs: Vec<DatasetId>,
}

impl ErrorRouter {
	pub fn load(db: &sled::Db, data: Arc<RwLock<Data>>) -> Result<ErrorRouter, DataserverError> {

		Ok(ErrorRouter {
			sessions: HashMap::new(),
			subs: HashMap::new(),
			clients_to_notify: NotifyChannels::load(&db)?, //keys = dataset_id+field_id
 			client_undisplayed_errors: db.open_tree("undisplayed errors")?,

			reported_errors: ReportedErrors::load(&db)?,
			data,
		})
	}
}

/// Make actor from `ChatServer`
impl Actor for ErrorRouter {
	/// We are going to use simple Context, we just need ability to communicate
	/// with other actors.
	type Context = Context<Self>;
}

///////////////////////////////////
