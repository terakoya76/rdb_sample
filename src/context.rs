use database::Database;
use allocators::allocator::Allocator;

#[derive(Debug, Clone)]
pub struct Context {
    pub db: Option<Database>,
    pub table_id_alloc: Box<Allocator>,
}
