use actix::*;
use serde::{Serialize, Deserialize};
use log::{warn, info, debug, trace};

use actix_web_actors::ws;

use chrono::{Utc};

use std::sync::{Arc,RwLock, Mutex};
use std::collections::HashMap;

use std::thread;
use std::sync::mpsc;
use std::sync::mpsc::sync_channel;
use chrono::DateTime;
use chrono::TimeZone; // We need the trait in scope to use Utc::timestamp().
use bytes::Bytes;

use crate::data_store;
use crate::data_store::data_router;
use bitspec::FieldId;
use super::Session;

use data_store::read_to_packets::{ReaderInfo, prepare_read_processing, read_into_packages};

pub struct TimesRange {
	pub start: DateTime<Utc>,
	pub stop: DateTime<Utc>,
}

impl Default for TimesRange {
	fn default() -> Self {
		TimesRange {
			start: Utc::now() - chrono::Duration::hours(12), //default timerange
			stop: Utc::now(),
		}
	}
}

// store data in here, it can then be accessed using self
pub struct WsSession {
	/// unique session id
	pub http_session_id: u16,
	pub ws_session_id: u16,
	//pub subscribed_fields: HashMap<data_store::DatasetId, Vec<SubbedField>>,
	pub compression_enabled: bool,
	pub timerange: TimesRange,

	pub selected_data: HashMap<data_store::DatasetId, Vec<FieldId>>,
	pub session: Arc<Mutex<Session>>,
	pub file_io_thread: Option<(thread::JoinHandle<()>, mpsc::Receiver<Vec<u8>>)>,
	
	pub data_router_addr: Addr<data_router::DataRouter>,
	pub data: Arc<RwLock<data_store::Data>>,

}

#[derive(Serialize, Deserialize)]
struct Trace {
    r#type: String,
    mode: String,
    //color: String,
    name: String,
}

#[derive(Serialize, Deserialize, Default)]
struct DataSetClientMeta {
	field_ids: Vec<FieldId>,
    traces_meta: Vec<Trace>,
    n_lines: u64,
    dataset_id: data_store::DatasetId,
}
//TODO check if static needed
impl Actor for WsSession {
	//type Context = ws::WebsocketContext<Self, DataRouterState>;
	type Context = ws::WebsocketContext<Self>;

	//fn started<T: InnerState>(&mut self, ctx: &mut Self::Context) {
	fn started(&mut self, ctx: &mut Self::Context) {

		let addr = ctx.address();
		self.data_router_addr
            .try_send(data_router::Connect {
                addr: addr.recipient(),
                ws_session_id: self.ws_session_id,
            })
			.unwrap();
	}

	fn stopping(&mut self, _ctx: &mut Self::Context) -> Running {
		// notify chat server
		self.data_router_addr
			.do_send(data_router::Disconnect { ws_session_id: self.ws_session_id });
		Running::Stop
	}
}


/// send messages to server if requested by dataserver
impl Handler<data_router::NewData> for WsSession {
	type Result = ();

	fn handle(&mut self, msg: data_router::NewData, ctx: &mut Self::Context) {
		trace!("client handler recieved signal there is new data");
		//recode data for this user
		let data_router::NewData{from_id, line, timestamp} = msg;

		let fields = self.selected_data.get(&from_id).unwrap();
		let data = self.data.read().unwrap();
		//let mut data = ctx.state().data.write().unwrap();
		let dataset = data.sets.get(&from_id).unwrap();
		info!("creating line for fields: {:?}, for set: {}",fields, from_id);
		let line = if self.compression_enabled {
			dataset.get_update(line, timestamp, fields, from_id)
		} else {
			dataset.get_update_uncompressed(line, timestamp, fields, from_id)
		};
		std::mem::drop(data);
		//send update
		debug!("sending update");
		ctx.binary(Bytes::from(line));
	}
}

impl WsSession {
	fn select_data(&mut self, args: Vec<&str>, compressed: bool) -> Result<(),core::num::ParseIntError>{
		if args.len() < 5 {return Ok(()) }
		self.timerange.start = Utc.timestamp(args[1].parse::<i64>()?/1000, (args[1].parse::<i64>()?%1000) as u32);
		self.timerange.stop = Utc.timestamp(args[2].parse::<i64>()?/1000, (args[2].parse::<i64>()?%1000) as u32);

		if let Ok(set_id) = args[3].parse::<data_store::DatasetId>() {
			//check if user has access to the requested dataset
			if let Some(fields_with_access) = self.session.lock().unwrap().db_entry.timeseries_with_access.get(&set_id){
				//parse requested fields
				if let Ok(field_ids) = args[4..]
					.iter()
					.map(|arg| arg.parse::<FieldId>())
					.collect::<Result<Vec<FieldId>,std::num::ParseIntError>>(){
					
					let mut subbed_fields = Vec::with_capacity(field_ids.len());
					for field_id in field_ids { 
						//prevent users requesting a field twice (this leads to an overflow later)
						if subbed_fields.contains(&field_id) {
							warn!("field was requested twice, ignoring duplicate");
						} else if fields_with_access.binary_search_by(|auth| auth.as_ref().cmp(&field_id)).is_ok() {
							subbed_fields.push(field_id);
						} else { 
							warn!("unautorised field requested");
							return Ok(());
						}
					}
					self.selected_data.insert(set_id, subbed_fields);
					self.compression_enabled = compressed;
				} else { warn!("invalid field requested") };
			} else { warn!("invalid dataset id"); }
		} else { warn!("no access to dataset"); }
		Ok(())
	}

	fn subscribe(&mut self){
		for set_id in self.selected_data.keys(){
			self.data_router_addr.do_send( data_router::SubscribeToSource {
				ws_session_id: self.ws_session_id,
				set_id: *set_id,
			});
		}
	}

	//TODO implement unsubscribe command
	fn unsubscribe(&mut self){
		unimplemented!();
	}

	fn send_decode_info(&self, args: Vec<&str>, ctx: &mut ws::WebsocketContext<Self>) {
		trace!("sending decode info to client");
		if args.len() < 2 {warn!("can not send decode info without setid"); return; }
		if let Ok(set_id) = args[1].parse::<data_store::DatasetId>() {
			if let Some(fields) = self.selected_data.get(&set_id){
				let data = self.data.read().unwrap();
				let dataset = data.sets.get(&set_id).unwrap();
				let decode_info = dataset.get_decode_info(fields);
				std::mem::drop(data);
				let decode_info = bincode::serialize(&decode_info).unwrap();
				ctx.binary(Bytes::from(decode_info));
			} else {
				warn!("tried access to unautorised or non existing dataset");
			}
		}
	}

	//TODO rethink entire data sending sys.
	// - needs to be able to try next dataset if lock cant be aquired
	// - only one thread may be used
	// - match ops that take client side time to server side heavy ops
	/////////////
	// ideas:
	// - put metadata sending in here
	/////////////
	// pitfalls:
	// - cant recieve packages from here
	// -
	/////////////
	// solution:
	// - prepare a list of data needed for reading
	// - start a thread in prepare_data (pass the above list)
	//    - try lock dont block
	//    - send to mpsc
	// 		- thread closes when mpsc closes or no more data availible
	//    - thread handle is stored in websocket session
	//    - no more then one thread can be started

	fn prepare_data(&mut self, ctx: &mut ws::WebsocketContext<Self>, args: Vec<&str>){
		let max_plot_points: u64 = if args.len() == 2 {
			args[1].parse().unwrap_or(100)
		} else {
			100
		};

		trace!("sending data to client");
		if self.file_io_thread.is_some() {
			warn!("already preparing data!");
			return;
		}

		let mut reader_infos: Vec<ReaderInfo> = Vec::with_capacity(self.selected_data.len());
		let mut client_metadata = Vec::with_capacity(self.selected_data.len());
		let data_handle = self.data.clone();
		let mut data = data_handle.write().unwrap();

		for (dataset_id, field_ids) in &self.selected_data {
			let mut dataset_client_metadata: DataSetClientMeta = Default::default();
			let dataset = data.sets.get_mut(dataset_id).unwrap();
			if let Some(read_state) = dataset.prepare_read(self.timerange.start, self.timerange.stop, field_ids) {
				//prepare for reading and calc number of bytes we will be sending
				let n_lines = std::cmp::min(read_state.numb_lines, max_plot_points);
				if let Some(reader_info) = prepare_read_processing(
					read_state, &dataset.timeseries, max_plot_points, *dataset_id) {
					reader_infos.push(reader_info);

					//prepare and send metadata
					for field_id in field_ids.iter().map(|id| *id) {
						let field = &dataset.metadata.fields[field_id as usize];
						dataset_client_metadata.traces_meta.push( Trace {
							r#type: "scattergl".to_string(),
							mode: "markers".to_string(),
							name: field.name.to_owned(),
						});
						dataset_client_metadata.field_ids.push(field_id);
					}
					dataset_client_metadata.n_lines = n_lines;
					dataset_client_metadata.dataset_id = *dataset_id;
					client_metadata.push(dataset_client_metadata);
				} else { warn!("could not setup read"); }
			} else { warn!("no data within given window"); }
		};
		std::mem::drop(data);

		let json = serde_json::to_string(&client_metadata).unwrap();
		println!("{:?}", json);

		ctx.text(json);

		//spawn file io thread
		let (tx, rx) = sync_channel(2);
		let thread = thread::spawn(move|| {
			read_into_packages(data_handle, tx, reader_infos); });
		self.file_io_thread = Some((thread, rx));
	}

	fn send_data(&mut self, ctx: &mut ws::WebsocketContext<Self>){
		if let Some((_thread, rx)) = self.file_io_thread.take() {
			while let Ok(buffer) = rx.recv() {
				if ctx.state().alive() {
					ctx.binary(Bytes::from(buffer ));
				} else {
					return;
				}
			} //send message to signal we are done sending data
			//third byte set to one signals this
			ctx.binary(Bytes::from(vec!(0u8,0,1,0)));
		}
	}
}

#[derive(Serialize)]
pub struct SetSliceDecodeInfo {
	pub field_lenghts: Vec<u8>,
	pub field_offsets: Vec<u8>,
	pub data_is_little_endian: bool,
}

/// Handler for `ws::Message`
impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WsSession {
	
	fn handle(
        &mut self,
        msg: Result<ws::Message, ws::ProtocolError>,
        ctx: &mut Self::Context,
    ) {
		// process websocket messages
		//println!("WS: {:?}", msg);
		match msg.unwrap() { // TODO FIXME should handle error here
			ws::Message::Text(text) => {
				let m = text.trim();
				if m.starts_with('/') {
					let args: Vec<&str> = m.split_whitespace().collect();
					println!("args: {:?}",args);

					match args[0] {
						//select uncompressed will case data to be send without compression
						//it is used until webassembly is relaiable
						"/select" => self.select_data(args, true).unwrap(),
						"/select_uncompressed" => self.select_data(args, false).unwrap(),

						"/sub" => self.subscribe(),
						"/unsub" => self.unsubscribe(),
						"/meta" => self.prepare_data(ctx, args),//prepares data and returns metadata to client
						"/RTC" => self.send_data(ctx),//client signals ready to recieve

						//for use with webassembly only
						"/decode_info" => self.send_decode_info(args, ctx),

						_ => ctx.text(format!("!!! unknown command: {:?}", m)),
					}
				}
			} //handle other websocket commands
			ws::Message::Ping(msg) => ctx.pong(&msg),
			ws::Message::Binary(bin) => ctx.binary(bin),
			ws::Message::Close(_) => {
				self.data_router_addr.do_send(data_router::Disconnect {ws_session_id: self.ws_session_id,});
				ctx.stop();
			}
			_ => (),
		}
	}
}

