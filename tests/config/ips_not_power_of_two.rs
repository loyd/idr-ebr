use idr_ebr::{Config, Idr};

struct InvalidConfig;
impl Config for InvalidConfig {
    const INITIAL_PAGE_SIZE: u32 = 31;
}

fn main() {
    let _ = Idr::<u64, InvalidConfig>::new();
}
