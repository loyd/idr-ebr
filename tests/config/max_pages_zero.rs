use idr_ebr::{Config, Idr};

struct InvalidConfig;
impl Config for InvalidConfig {
    const MAX_PAGES: u32 = 0;
}

fn main() {
    let _ = Idr::<u64, InvalidConfig>::new();
}
