//maximum of package lines to read for one request, increasing this dramatically increases virt memory usage,
//large values may give memory allocation panics.
//pub const MAX_BYTES_PER_PACKAGE: usize = 80;
//pub const MAX_BYTES_PER_PACKAGE: usize = 264;

pub const MAX_READ_MEMORY: usize = 10_000; //10Kb
pub const MAX_PACKAGE_SIZE: usize = 10_000; //10Kb must be smaller or equal to max package size


pub const FORCE_CERT_REGEN: bool = false;
