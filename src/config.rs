//maximum of package lines to read for one request, increasing this dramatically increases virt memory usage,
//large values may give memory allocation panics.
pub const MAX_LINES_PER_PACKAGE: usize = 10_000;
