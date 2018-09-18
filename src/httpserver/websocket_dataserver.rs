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

#[derive(Message)]
pub struct Message(pub String);

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
