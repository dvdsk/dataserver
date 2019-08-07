#[repr(u8)]
#[derive(FromPrimitive, ToPrimitive)]
enum RemoteError {
    CANT_FIND_BME680 = 0,
    CANT_CONFIGURE_MHZ19,
    MAX44009_LIB_ERROR,

    CANT_OPEN_FILE_FOR_WRITING,
    CANT_WRITE_TO_FILE,
    FILE_DOES_NOT_EXIST,
    CANT_OPEN_FILE_FOR_READING,
    FILE_HAS_INCORRECT_SIZE,
    READ_MORE_THEN_PARAMS,
    
    INVALID_SERVER_RESPONSE,
}

impl std::convert::from<u8> for RemoteError {
    fn from(raw: u8) -> Self {
        Self::from_u8(raw)
    }
}

impl fmt::Display for RemoteError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // The `f` value implements the `Write` trait, which is what the
        // write! macro is expecting. Note that this formatting ignores the
        // various flags provided to format strings.
        match self {
            RemoteError::CANT_FIND_BME680 => write!(f,"could not find connected bme680"),
            RemoteError::CANT_CONFIGURE_MHZ19 => write!(f,"could not find connected mhz19"),
            RemoteError::MAX44009_LIB_ERROR => write!(f,"could not find connected max44009"),

            RemoteError::CANT_OPEN_FILE_FOR_WRITING => write!(f,"could not open wifi paramater file for writing"),
            RemoteError::CANT_WRITE_TO_FILE => write!(f,"could not write to wifi paramater file"),
            RemoteError::CANT_OPEN_FILE_FOR_READING => write!(f,"could not open wifi paramater file for reading"),
            RemoteError::FILE_HAS_INCORRECT_SIZE => write!(f,"wifi paramaters file is corrupted (has incorrect size)"),
            RemoteError::READ_MORE_THEN_PARAMS => write!(f,"wifi paramaters file is corrupted (read more then correct size)"),
            
            RemoteError::INVALID_SERVER_RESPONSE => write!(f,"could not push data to server, incorrect server response"),
        }
    }
}