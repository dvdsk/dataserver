use std::sync::{Arc, RwLock, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::io::Read;

use std::task::{Waker, Context, Poll};
use std::future::Future;
use std::pin::Pin;

use dialoguer::{Select};
use crate::databases::{PasswordDatabase, UserDatabase, UserLookup};
use crate::data_store::{Data};

mod user;
mod data;

fn main_menu() -> usize {
    Select::new()
        .paged(true)
        .item("shutdown")
        .item("modify/remove users")
        .item("export/archive datasets")
        .item("add user")
        .item("add dataset")
        .default(0)
        .interact().unwrap()
}

fn command_line_interface(data: Arc<RwLock<Data>>, 
                          mut passw_db: PasswordDatabase, 
						  mut user_db: UserDatabase,
						  lookup: UserLookup){

    loop {
        match main_menu(){
            0 => break,
            1 => user::menu(&mut user_db, &lookup, &mut passw_db, &data),
            2 => data::choose_dataset(&mut user_db, &data),
            3 => user::add_user(&mut user_db, &mut passw_db),
            4 => data::add_set(&data),
            _ => panic!(),

        }
    }
}

pub struct Menu {
	_thread: thread::JoinHandle<()>,
	waker: Arc<Mutex<Option<Waker>>>,
	done: Arc<AtomicBool>,
}

impl Menu {
	pub fn gui(data: Arc<RwLock<Data>>, passw_db: PasswordDatabase, 
		web_user_db: UserDatabase, lookup: UserLookup) -> Self {
		
		let waker = Arc::new(Mutex::new(None));
		let done = Arc::new(AtomicBool::new(false));
		Self{
			done: done.clone(),
			waker: waker.clone(),
			_thread: thread::spawn(move || {
				command_line_interface(
					data, 
					passw_db, 
					web_user_db, 
					lookup);
				done.store(true, Ordering::SeqCst);
				if let Some(waker) = waker.lock().unwrap().take(){
					waker.wake();
				}
			}),
		}
	}

	pub fn simple() -> Self {
		let waker = Arc::new(Mutex::new(None));
		let done = Arc::new(AtomicBool::new(false));
		Self{
			done: done.clone(),
			waker: waker.clone(),
			_thread: thread::spawn(move || {
				println!("press enter to stop");
				std::io::stdin().read_exact(&mut [0]).unwrap();
				done.store(true, Ordering::SeqCst);
				if let Some(waker) = waker.lock().unwrap().take(){
					waker.wake();
				}
			}),
		}
	}
}

impl Future for Menu {
	type Output = ();
	
	fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
		if self.done.load(Ordering::SeqCst) {
			Poll::Ready(())
		} else {
			self.waker.lock().unwrap().replace(cx.waker().clone());
			Poll::Pending
		}
	}
}