//maximum of package lines to read for one request, increasing this dramatically increases virt memory usage,
//large values may give memory allocation panics.
//pub const MAX_BYTES_PER_PACKAGE: usize = 80;
pub const MAX_BYTES_PER_PACKAGE: usize = 264;
//pub const MAX_BYTES_PER_PACKAGE: usize = 10_000;

pub const FORCE_CERT_REGEN: bool = false;
