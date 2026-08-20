use zkr::store::MemoryDb;
use std::time::Instant;

fn main() {
    let _db = MemoryDb::open(":memory:");
}
