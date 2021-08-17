pub use bitspec;
use bitspec::FieldId;
use std::cmp::Ordering;
use serde::{Serialize, Deserialize};

pub type DataSetId = u16;
pub type UserId = u64;

pub struct User {
    pub name: String,
    pub last_login: String,
    pub telegram_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq)]
pub enum Authorisation {
	Owner(bitspec::FieldId),
	Reader(bitspec::FieldId),
}

impl Ord for Authorisation {
	fn cmp(&self, other: &Self) -> Ordering {
		FieldId::from(self).cmp(&FieldId::from(other))
	}
}

impl PartialOrd for Authorisation {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		Some(self.cmp(other))
	}
}

impl PartialEq for Authorisation {
	fn eq(&self, other: &Self) -> bool {
		FieldId::from(self) == FieldId::from(other)
	}
}

impl std::hash::Hash for Authorisation {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        bitspec::FieldId::from(self).hash(state);
    }
}

impl AsRef<bitspec::FieldId> for Authorisation {
	fn as_ref(&self) -> &bitspec::FieldId {
		match self {
			Self::Owner(id) => id,
			Self::Reader(id) => id,
		}
	}
}

impl std::convert::From<&Authorisation> for bitspec::FieldId {
	fn from(auth: &Authorisation) -> bitspec::FieldId {
		match auth {
			Authorisation::Owner(id) => *id,
			Authorisation::Reader(id) => *id,
		}
	}
}
