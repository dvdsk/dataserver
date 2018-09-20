extern crate actix;
extern crate actix_web;
extern crate rand;

use self::actix::prelude::*;
use self::rand::{Rng, ThreadRng};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};


///// New chat session is created
//#[derive(Message)]
//#[rtype(usize)]
//pub struct Connect {
	//pub addr: Recipient<Message>,
//}

pub enum data_type {
	light,
	humidity,
	temperature,
}

#[derive(Message)]
pub struct Message(pub String);

/// New chat session is created
#[derive(Message)]
#[rtype(usize)]
pub struct Connect {
    pub addr: Recipient<Message>,
}

#[derive(Message)]
#[rtype(usize)]
pub struct NewData {
    pub from: data_type,
}

impl Handler<NewData> for DataServer {
    //type Result = usize;
    type Result = usize;

    fn handle(&mut self, msg: NewData, _: &mut Context<Self>) -> Self::Result {
        println!("send that there is new data");
		1
    }
}

impl Handler<Connect> for DataServer {
    type Result = usize;

    fn handle(&mut self, msg: Connect, _: &mut Context<Self>) -> Self::Result {
        println!("Someone joined");

        // register session with random id
        let id = self.rng.borrow_mut().gen::<usize>();
        self.sessions.insert(id, msg.addr);

        // auto join session to Main room
        self.rooms.get_mut(&"Main".to_owned()).unwrap().insert(id);

        // send id back
        id
    }
}

pub struct DataServer {
    sessions: HashMap<usize, Recipient<Message>>,
    rooms: HashMap<String, HashSet<usize>>,
    rng: RefCell<ThreadRng>,
}

impl Default for DataServer {
    fn default() -> DataServer {
        // default room
        let mut rooms = HashMap::new();
        rooms.insert("Main".to_owned(), HashSet::new());

        DataServer {
            sessions: HashMap::new(),
            rooms: rooms,
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
