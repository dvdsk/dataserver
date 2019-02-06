use self::actix::Addr;
use self::actix::*;

extern crate actix;
extern crate actix_net;
extern crate actix_web;
extern crate chrono;
extern crate smallvec;
extern crate bincode;
extern crate serde_derive;

extern crate byteorder;
extern crate crossbeam_utils;

use self::byteorder::{LittleEndian, WriteBytesExt};

use self::actix_web::{Binary,ws};
use self::chrono::{Utc, Duration};
use super::futures::Future;

use std::sync::{Arc,RwLock};
use std::collections::HashMap;

use std::thread;
use std::sync::mpsc;
use std::sync::mpsc::sync_channel;

use super::timeseries_interface;
use super::websocket_data_router;
use super::DataServerState;
use super::super::config;
use crate::httpserver::InnerState;

// store data in here, it can then be accessed using self
pub struct WsSession<'a,T: InnerState> {
	/// unique session id
	pub http_session_id: u16,
	pub ws_session_id: u16,
	//pub subscribed_fields: HashMap<timeseries_interface::DatasetId, Vec<SubbedField>>,
	pub compression_enabled: bool,
	pub selected_data: HashMap<timeseries_interface::DatasetId, Vec<timeseries_interface::FieldId>>,
	pub timeseries_with_access: Arc<RwLock<HashMap<timeseries_interface::DatasetId, Vec<timeseries_interface::Authorisation>>>>,
	pub file_io_thread: Option<(thread::JoinHandle<()>, mpsc::Receiver<Vec<u8>>)>,
	pub phantom: std::marker::PhantomData<&'a T>,
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
		field_ids: Vec<timeseries_interface::FieldId>,
    traces_meta: Vec<Trace>,
    n_lines: usize,
    dataset_id: timeseries_interface::DatasetId,
}

impl<T: InnerState+'static> Actor for WsSession<'static,T> {
	//type Context = ws::WebsocketContext<Self, DataServerState>;
	type Context = ws::WebsocketContext<Self, T>;

	//fn started<T: InnerState>(&mut self, ctx: &mut Self::Context) {
	fn started(&mut self, ctx: &mut Self::Context) {
		// register self in chat server. `AsyncContext::wait` register
		// future within context, but context waits until this future resolves
		// before processing any other events.
		// HttpContext::state() is instance of WsChatSessionState, state is shared
		// across all routes within application

		let addr = ctx.address();
		ctx.state().inner_state()
            .websocket_addr
            .send(websocket_data_router::Connect {
                addr: addr.recipient(),
                ws_session_id: self.ws_session_id,
            })
            .wait().unwrap();
	}

	fn stopping(&mut self, ctx: &mut Self::Context) -> Running {
		// notify chat server
		ctx.state().inner_state()
			.websocket_addr
			.do_send(websocket_data_router::Disconnect { ws_session_id: self.ws_session_id });
		Running::Stop
	}
}


/// send messages to server if requested by dataserver
impl<T: InnerState+'static> Handler<websocket_data_router::NewData> for WsSession<'static,T> {
	type Result = ();

	fn handle(&mut self, msg: websocket_data_router::NewData, ctx: &mut Self::Context) {
		trace!("client handler recieved signal there is new data");
		//recode data for this user
		let websocket_data_router::NewData{from_id, line, timestamp} = msg;

		let fields = self.selected_data.get(&from_id).unwrap();
		let data = ctx.state().inner_state().data.read().unwrap();
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
		ctx.binary(Binary::from(line));
	}
}

fn divide_ceil(x: usize, y: usize) -> usize{
	(x + y - 1) / y
}

/*
impl<'a,T: InnerState> WsSession<'a,T> {
	
	fn select_data(&mut self, args: Vec<&str>, compressed: bool){
		if args.len() < 3 {return }
		if let Ok(set_id) = args[1].parse::<timeseries_interface::DatasetId>() {
			//check if user has access to the requested dataset
			if let Some(fields_with_access) = self.timeseries_with_access.read().unwrap().get(&set_id){
				//parse requested fields
				if let Ok(field_ids) = args[2..]
					.into_iter()
					.map(|arg| arg.parse::<timeseries_interface::FieldId>())
					.collect::<Result<Vec<timeseries_interface::FieldId>,std::num::ParseIntError>>(){
					
					let mut subbed_fields = Vec::with_capacity(field_ids.len());
					for field_id in field_ids { 
						//prevent users requesting a field twice (this leads to an overflow later)
						if subbed_fields.contains(&field_id) {
							warn!("field was requested twice, ignoring duplicate");
						} else if fields_with_access.binary_search_by(|auth| auth.as_ref().cmp(&field_id)).is_ok() {
							subbed_fields.push(field_id);
						} else { 
							warn!("unautorised field requested");
							return;
						}
					}
					self.selected_data.insert(set_id, subbed_fields);
					self.compression_enabled = compressed;
				} else { warn!("invalid field requested") };
			} else { warn!("invalid dataset id"); }
		} else { warn!("no access to dataset"); }
	}

	fn subscribe(&mut self, websocket_addr: &Addr<websocket_data_router::DataServer>){
		for set_id in self.selected_data.keys(){
			websocket_addr.do_send( websocket_data_router::SubscribeToSource {
				ws_session_id: self.ws_session_id,
				set_id: *set_id,
			});
		}
	}

	//TODO implement unsubscribe command
	fn unsubscribe(&mut self, _websocket_addr: &Addr<websocket_data_router::DataServer>){
		unimplemented!();
	}

	fn send_decode_info(&self, args: Vec<&str>, ctx: &mut ws::WebsocketContext<Self, T>) {
		trace!("sending decode info to client");
		if args.len() < 2 {warn!("can not send decode info without setid"); return; }
		if let Ok(set_id) = args[1].parse::<timeseries_interface::DatasetId>() {
			if let Some(fields) = self.selected_data.get(&set_id){
				let data = ctx.state().data.read().unwrap();
				let dataset = data.sets.get(&set_id).unwrap();
				let decode_info = dataset.get_decode_info(fields);
				std::mem::drop(data);
				let decode_info = bincode::serialize(&decode_info).unwrap();
				ctx.binary(Binary::from(decode_info));
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

	fn prepare_data(&mut self, ctx: &mut ws::WebsocketContext<Self, T>){
		trace!("sending data to client");
		if self.file_io_thread.is_some() {
			warn!("already preparing data!");
			return;
		}

		let now = Utc::now();
		//let t_start= now - Duration::hours(196);
		let t_start= now - Duration::days(20);
		let t_end = Utc::now();

		let mut reader_infos: Vec<ReaderInfo> = Vec::with_capacity(self.selected_data.len());
		let mut client_metadata = Vec::with_capacity(self.selected_data.len());
		let data_handle = ctx.state().data.clone();
		let mut data = data_handle.write().unwrap();

		for (dataset_id, field_ids) in  &self.selected_data {
			let mut dataset_client_metadata: DataSetClientMeta = Default::default();
			let dataset = data.sets.get_mut(dataset_id).unwrap();
			let read_state = dataset.prepare_read(t_start,t_end, field_ids).unwrap();

			//prepare for reading and calc number of bytes we will be sending
			const PACKAGE_HEADER_SIZE: usize = 8;
			let numb_lines = read_state.numb_lines;
			let bytes_to_send = read_state.numb_lines*(read_state.decoded_line_size+std::mem::size_of::<f64>())+PACKAGE_HEADER_SIZE;
			let n_packages = divide_ceil(bytes_to_send, config::MAX_BYTES_PER_PACKAGE);

			reader_infos.push(ReaderInfo {
				dataset_id: *dataset_id,
				n_packages,
				read_state,
			});

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
			dataset_client_metadata.n_lines = numb_lines;
			dataset_client_metadata.dataset_id = *dataset_id;
			client_metadata.push(dataset_client_metadata);
		};
		std::mem::drop(data);

		let json = serde_json::to_string(&client_metadata).unwrap();
		println!("{:?}", json);
		//sample output: //TODO adjust plot.js to handle new style of sending metadata
		// "[
		// {
		// \"id_info\":[{\"dataset_id\":0,\"field_id\":0},{\"dataset_id\":0,\"field_id\":1}],
		// \"lines\":[{\"type\":\"scattergl\",\"mode\":\"markers\",\"name\":\"Temperature\"},
		//            {\"type\":\"scattergl\",\"mode\":\"markers\",\"name\":\"Humidity\"}],
		// \"n_lines\":3
		//  }
		//  ]"

		ctx.text(json);


		//spawn file io thread
		let (tx, rx) = sync_channel(2);
		let thread = thread::spawn(move|| { read_into_buffers(data_handle, tx, reader_infos); });
		self.file_io_thread = Some((thread, rx));
	}

	fn send_data(&mut self, ctx: &mut ws::WebsocketContext<Self, T>){
		if let Some((_thread, rx)) = self.file_io_thread.take() {
			while let Ok(buffer) = rx.recv() {
				if ctx.connected() {
					ctx.binary(Binary::from(buffer ));
				} else {
					return;
				}
			} //send message to signal we are done sending data
			//third byte set to one signals this
			ctx.binary(Binary::from(vec!(0u8,0,1,0)));
		}
	}
}
*/
struct ReaderInfo {
	dataset_id: timeseries_interface::DatasetId,
	n_packages: usize,
	read_state: timeseries_interface::ReadState,
}

fn read_into_buffers(data_handle: Arc<RwLock<timeseries_interface::Data>>, tx: mpsc::SyncSender<Vec<u8>>, reader_infos: Vec<ReaderInfo>){

	for ReaderInfo {dataset_id, n_packages, mut read_state} in reader_infos {
		for package_numb in (0..n_packages).rev() {
			let mut data = data_handle.write().unwrap(); //write needed as file needs write
			let dataset = data.sets.get_mut(&dataset_id).unwrap();

			let buffer = dataset.get_data_chunk_uncompressed(
				&mut read_state,
				config::MAX_BYTES_PER_PACKAGE,
				package_numb as u16,
				dataset_id
			).unwrap();//FIXME handle possible error!!!

			std::mem::drop(dataset);
			std::mem::drop(data);

			//block if parent thread has not yet send the previouse n packets
			//break and end thread if something went wrong
			if tx.send(buffer).is_err() {break; };
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
impl<T: InnerState+'static> StreamHandler<ws::Message, ws::ProtocolError> for WsSession<'static,T> {
	
	fn handle(&mut self, msg: ws::Message, ctx: &mut Self::Context) {
		// process websocket messages
		//println!("WS: {:?}", msg);
		match msg {
			ws::Message::Text(text) => {
				let m = text.trim();
				if m.starts_with('/') {
					let args: Vec<&str> = m.split_whitespace().collect();
					//println!("args: {:?}",args);
					/*
					match args[0] {
						//select uncompressed will case data to be send without compression
						//it is used until webassembly is relaiable
						"/select" => self.select_data(args, true),
						"/select_uncompressed" => self.select_data(args, false),

						"/sub" => self.subscribe(&ctx.state().websocket_addr),
						"/meta" => self.prepare_data(ctx),
						"/RTC" => self.send_data(ctx),//client signals ready to recieve

						//for use with webassembly only
						"/decode_info" => self.send_decode_info(args, ctx),

						_ => ctx.text(format!("!!! unknown command: {:?}", m)),
					}*/
				}
			} //handle other websocket commands
			ws::Message::Ping(msg) => ctx.pong(&msg),
			ws::Message::Binary(bin) => ctx.binary(bin),
			ws::Message::Close(_) => {
				ctx.state().inner_state().websocket_addr.do_send(websocket_data_router::Disconnect {ws_session_id: self.ws_session_id,});
				ctx.stop();
			}
			_ => (),
		}
	}
}

