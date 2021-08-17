use actix::prelude::*;
use log::{debug, trace};
use std::sync::{Arc, RwLock};

use bincode;
use chrono::{offset::Utc, DateTime};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::data_store::{Data, DatasetId, FieldId};
use crate::error::DataserverError;

mod sensor_errors;
use sensor_errors::RemoteError;

/*
	Errors for sensors and custom system

	msg:
	---------------------------------------------------------
	-- dataset_id [u16]-- field_id [u16] -- error code [u8]--
	---------------------------------------------------------

	dataset_id zero (0) is reserved for custom errors that should be reported to the web server.
	field_id zero (255) is reserved for errors not relevant to a field (=sensor) but the entire dataset
	the first 124 error codes are generic errors, from 125 and higher are specific to the sensor
*/

pub type ErrorCode = u8;

struct ReportedErrors {
	tree: sled::Tree,
}

impl ReportedErrors {
	fn load(db: &sled::Db) -> Result<Self, DataserverError> {
		Ok(Self {
			tree: db.open_tree("reported_errors")?,
		})
	}

	//return true if this error was reported within a day, if it was not remembers the
	//error as reported now
	fn recently_reported(&mut self, msg: &NewError) -> Result<bool, DataserverError> {
		//errors are stored based on 32bit key these are sorted as:
		//-----3-------2--------------1-----------------0---------- (byte)
		//-- dataset_id [u16]-- field_id [u16] -- error code [u8]--
		//---------------------------------------------------------
		//dataset_id in big endian representation, this allows us
		//to use ranges in database querys
		let key = msg.to_error_specific_key().to_be_bytes();

		if let Some(last_reported) = self.tree.get(&key).unwrap() {
			let last_reported: DateTime<Utc> = bincode::deserialize(&last_reported)?;
			if last_reported.signed_duration_since(Utc::now()) > chrono::Duration::days(1) {
				self.tree.insert(&key, bincode::serialize(&Utc::now())?)?;
				Ok(true)
			} else {
				Ok(false)
			}
		} else {
			self.tree.insert(&key, bincode::serialize(&Utc::now())?)?;
			Ok(false)
		}
	}
}

struct NotifyChannels {
	tree: sled::Tree,
}

#[derive(Serialize, Deserialize)]
struct NotifyOptions {
	email: Option<String>,
	telegram: Option<()>,
}

impl NotifyChannels {
	fn load(db: &sled::Db) -> Result<Self, DataserverError> {
		Ok(Self {
			tree: db.open_tree("notify_channels")?,
		})
	}

	//return true if this error was reported within a day, if it was not remembers the
	//error as reported now
	fn should_notify(
		&mut self,
		msg: &NewError,
	) -> Result<Option<Vec<NotifyOptions>>, DataserverError> {
		//errors are stored based on 32bit key these are sorted as:
		//-----3-------2--------------1-----------------0---------- (byte)
		//-- dataset_id [u16]-- field_id [u16] -- error code [u8]--
		//---------------------------------------------------------
		//dataset_id in big endian representation, this allows us
		//to use ranges in database querys

		let key = msg.to_field_specific_key().to_be_bytes();

		if let Some(to_notify) = self.tree.get(&key)? {
			let to_notify: Vec<NotifyOptions> = bincode::deserialize(&to_notify)?;
			Ok(Some(to_notify))
		} else {
			Ok(None)
		}
	}
}

pub struct Clientinfo {
	addr: Recipient<NewFormattedError>,
	subs: Vec<FieldSpecificKey>,
}

type ClientSessionId = u16;
//Errors are grouped by dataset
//TODO add general error log
pub struct ErrorRouter {
	sessions: HashMap<ClientSessionId, Clientinfo>,
	ws_subs: HashMap<FieldSpecificKey, HashSet<ClientSessionId>>,

	//TODO speed this up dramatically by using an in memory representation for reads and updating Db on write
	clients_to_notify: NotifyChannels,     //keys = dataset_id+field_id
	client_undisplayed_errors: sled::Tree, // display as soon as client loads/connects
	reported_errors: ReportedErrors,

	data: Arc<RwLock<Data>>,
}

#[derive(Message, Clone)]
#[rtype(result = "()")]
pub struct NewError {
	pub dataset_id: DatasetId,
	pub field_ids: Vec<FieldId>,
	pub error_code: ErrorCode,
	pub timestamp: DateTime<Utc>,
}

pub fn to_field_specific_key(dataset_id: DatasetId, field_id: FieldId) -> FieldSpecificKey {
	let mut key: FieldSpecificKey = 0;
	key |= (dataset_id as u32) << 16;
	key |= (field_id as u32) << 8;
	key
}

type ErrorSpecificKey = u32;
pub type FieldSpecificKey = u32;
impl NewError {
	fn to_error_specific_key(&self) -> ErrorSpecificKey {
		let mut key: ErrorSpecificKey = 0;
		key |= (self.dataset_id as u32) << 16;
		key |= (self.field_ids[0] as u32) << 8;
		key |= self.error_code as u32;
		key
	}
	fn to_field_specific_key(&self) -> FieldSpecificKey {
		to_field_specific_key(self.dataset_id, self.field_ids[0])
	}
}

///Formats error codes:
/// if field_id and dataset_id > 0
///     [time] data collection error in set: [dataset name]([dataset description]) specificly [field_id name] reports: [error code explanatin]
/// if the field_id == 0
///     [time] data collection error in set: [dataset name]([dataset description]) error message: [error code explanatin]
/// if the dataset_id == 0 then this is a system error and is reported on as follows:
///     [time] system error [error code explanation]

fn format_error_code(data: &Arc<RwLock<Data>>, msg: &NewError) -> Result<String, ()> {
	//TODO add timestamp
	let error = RemoteError::from(msg.error_code);
	if msg.dataset_id == 0 {
		return Ok(format!(
			"{time} system error occured: {error}",
			time = msg.timestamp,
			error = error
		)
		.to_string());
	}

	if let Some(dataset) = data.read().unwrap().sets.get(&msg.dataset_id) {
		let metadata = &dataset.metadata;
		if msg.field_ids[0] == u8::max_value() {
			Ok(format!("{time} error during data collection, {dataset_name}({dataset_description}) reports: {error}",
				time=msg.timestamp,
				dataset_name=metadata.name,
				dataset_description = metadata.description,
				error=error,
				).to_string())
		} else {
			let mut field_names = String::new();
			for field_id in &msg.field_ids {
				if let Some(field) = metadata.fields.get(*field_id as usize) {
					field_names.push_str(&format!("{},", field.name));
					field_names.pop();
				} else {
					return Err(());
				}
			}
			Ok(format!("{time} error during data collection, {field_name} in {dataset_name}({dataset_description}) reports: {error}",
				time=msg.timestamp,
				field_name=field_names,
				dataset_name=metadata.name,
				dataset_description = metadata.description,
				error=error,
				).to_string())
		}
	} else {
		Err(())
	}
}

impl Handler<NewError> for ErrorRouter {
	type Result = ();

	fn handle(&mut self, msg: NewError, _: &mut Context<Self>) -> Self::Result {
		if self.reported_errors.recently_reported(&msg).unwrap() {
			return;
		}

		let error_msg = format_error_code(&self.data, &msg).unwrap();

		//get a list of clients connected interested in this dataset
		if let Some(subs) = self.ws_subs.get(&msg.to_field_specific_key()) {
			debug!("subs: {:?}", subs);
			for client_session_id in subs.iter() {
				// foward new data message to actor that maintains the
				// websocket connection with this client.
				let client_websocket_handler = &self.sessions.get(client_session_id).unwrap().addr;
				client_websocket_handler
					.do_send(NewFormattedError {
						error_message: error_msg.clone(),
					})
					.unwrap();
			}
		}
		//fetch the list of notification channels from
		if let Some(to_notify) = self.clients_to_notify.should_notify(&msg).unwrap() {
			for notify_option in to_notify {
				if let Some(_mail_adress) = notify_option.email {
					unimplemented!();
				}
				if let Some(_telegram) = notify_option.telegram {
					unimplemented!();
				}
			}
		}
	}
}
#[derive(Message)]
#[rtype(result = "()")]
pub struct NewFormattedError {
	pub error_message: String,
}

/// New chat session is created
#[derive(Message)]
#[rtype(result = "()")]
pub struct Connect {
	pub addr: Recipient<NewFormattedError>,
	pub ws_session_id: u16,
	pub subscribed_errors: Vec<FieldSpecificKey>,
}

impl Handler<Connect> for ErrorRouter {
	type Result = ();

	fn handle(&mut self, msg: Connect, _: &mut Context<Self>) -> Self::Result {
		// register session with id
		let id = msg.ws_session_id;
		let mut subs = Vec::new();
		// sub to errors
		for field_specific_key in msg.subscribed_errors {
			subs.push(field_specific_key);
			if let Some(subscribed_clients) = self.ws_subs.get_mut(&field_specific_key) {
				subscribed_clients.insert(msg.ws_session_id);
			} else {
				let mut subscribed_clients = HashSet::new();
				subscribed_clients.insert(msg.ws_session_id);
				self.ws_subs.insert(field_specific_key, subscribed_clients);
			}
		}

		self.sessions.insert(
			id,
			Clientinfo {
				addr: msg.addr,
				subs,
			},
		);
	}
}

#[derive(Message)]
#[rtype(result = "()")]
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
				if let Some(subbed_clients) = self.ws_subs.get_mut(&sub) {
					subbed_clients.remove(&msg.ws_session_id);
					trace!("removed client from: sub:{:?} ", sub);
				}
			}
		}
	}
}

impl ErrorRouter {
	pub fn load(db: &sled::Db, data: Arc<RwLock<Data>>) -> Result<ErrorRouter, DataserverError> {
		Ok(ErrorRouter {
			sessions: HashMap::new(),
			ws_subs: HashMap::new(),
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
