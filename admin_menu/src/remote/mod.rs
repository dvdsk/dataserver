use std::net::{TcpStream, SocketAddr};
use dataserver::bitspec;
use dataserver::{DataSetId, UserId, Authorisation, User};

pub struct Connection {
    stream: TcpStream,
}

impl Connection {
    pub fn from_port(port: u16) -> Self {
        let addr = SocketAddr::from(([127,0,0,1], port));
        let stream = TcpStream::connect(addr).unwrap();
        Self { stream }
    }

    pub fn ensure_template(&self) {
    }
    pub fn get_specs(&self) -> Vec<String> {
        todo!()
    }
    pub fn add_dataset(&self, file_name: &str) -> Result<DataSetId, String> {
        todo!()
    }
    pub fn get_dataset_lists(&self) -> (Vec<String>, Vec<DataSetId>) {
        todo!()
    }
    pub fn get_metadata(&self, set_id: DataSetId) -> bitspec::FixedLine {
        todo!()
    }
    pub fn export(&self, set_id: DataSetId) {
    }
    pub fn get_user_lists(&self) -> (Vec<String>, Vec<u64>) {
        todo!();
    }
    pub fn get_user_by_id(&self, user_id: UserId) -> User {
        todo!()
    }
    pub fn remove_user(&self, user_id: UserId) {
    }
    pub fn set_password(&self, user_id: UserId, new_password: &str) {
    }
    pub fn add_user(&self, name: &str, password: &str) -> Result<(), String> {
        todo!()
    }
    pub fn change_user_name(&self, user_id: UserId, new_name: &str) -> Result<(), String> {
        todo!()
    }
    pub fn change_telegram_id(&self, user_id: UserId, new_id: &str) -> Result<(), String> {
        todo!()
    }
    pub fn acessible_datasets(&self, user_id: UserId) -> (Vec<String>, Vec<DataSetId>) {
        todo!()
    }
    pub fn inaccesible_datsets(&self) -> (Vec<String>, Vec<DataSetId>) {
        todo!()
    }
    // pub fn add_access(&self, set_id: DataSetId, authorized_fields) {
    // }
    pub fn dataset_fields(&self, set_id: DataSetId) -> Vec<bitspec::Meta> {
        todo!()
    }
    pub fn accessible_fields(&self, set_id: DataSetId) -> Vec<Authorisation> {
        todo!()
    }
    pub fn make_fields_accesible(&self, set_id: DataSetId, fields: Vec<Authorisation>) {
        todo!()
    }
    pub fn remove_dataset_access(&self, set_id: DataSetId) {
    }
}
