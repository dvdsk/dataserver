use num_derive::FromPrimitive;
use num_traits::FromPrimitive;

#[repr(u8)]
#[derive(FromPrimitive)]
pub enum RemoteError {
    CantFindBme680 = 0,
    CantConfigureMhz19,
    Max44009LibError,

    CantOpenFileForWriting,
    CantOpenFileForReading,
    CantWriteToFile,
    FileHasIncorrectSize,
    ReadMoreThenParams,
    
    InvalidServerResponse,
}

impl std::convert::From<u8> for RemoteError {
    fn from(raw: u8) -> Self {
        Self::from_u8(raw).unwrap()
    }
}

impl std::fmt::Display for RemoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        // The `f` value implements the `Write` trait, which is what the
        // write! macro is expecting. Note that this formatting ignores the
        // various flags provided to format strings.
        match self {
            RemoteError::CantFindBme680 => write!(f,"could not find connected bme680"),
            RemoteError::CantConfigureMhz19 => write!(f,"could not find connected mhz19"),
            RemoteError::Max44009LibError => write!(f,"could not find connected max44009"),

            RemoteError::CantOpenFileForWriting => write!(f,"could not open wifi paramater file for writing"),
            RemoteError::CantWriteToFile => write!(f,"could not write to wifi paramater file"),
            RemoteError::CantOpenFileForReading => write!(f,"could not open wifi paramater file for reading"),
            RemoteError::FileHasIncorrectSize => write!(f,"wifi paramaters file is corrupted (has incorrect size)"),
            RemoteError::ReadMoreThenParams => write!(f,"wifi paramaters file is corrupted (read more then correct size)"),
            
            RemoteError::InvalidServerResponse => write!(f,"could not push data to server, incorrect server response"),
        }
    }
}