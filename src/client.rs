use context::Context;
use database::DatabaseError;
use meta::table_info::{TableInfo, TableInfoError};
use meta::column_info::ColumnInfo;
use columns::range::Range;
use tables::memory_table::MemoryTable;
use tables::field::Field;
use allocators::allocator::Allocator;

use parser::statement::*;
use parser::parser::{Parser, ParseError};
use executors::memory_table_scan::MemoryTableScanExec;
use executors::projection::ProjectionExec;
use executors::selection::SelectionExec;
use executors::selector::*;
use executors::join::NestedLoopJoinExec;

#[derive(Debug)]
pub struct Client {
   pub ctx: Context,
}

impl Client {
    pub fn new(ctx: Context) -> Client {
        Client {
            ctx: ctx,
        }
    }

    pub fn handle_query(&mut self, query: &str) -> Result<(), ClientError> {
        let mut parser: Parser = Parser::new(query);
        let stmt = try!(parser.parse());
        match stmt.clone() {
            Statement::DDL(stmt) => exec_ddl(&mut self.ctx, stmt),
            Statement::DML(stmt) => exec_dml(&mut self.ctx, stmt),
        }
    }
}

pub fn exec_ddl(ctx: &mut Context, stmt: DDL) -> Result<(), ClientError> {
    match stmt {
        DDL::Create(stmt) => exec_create(ctx, stmt),
    }
}

pub fn exec_create(ctx: &mut Context, stmt: CreateStmt) -> Result<(), ClientError> {
    match stmt {
        CreateStmt::Table(stmt) => create_table_stmt(ctx, stmt),
    }
}

pub fn create_table_stmt(ctx: &mut Context, stmt: CreateTableStmt) -> Result<(), ClientError> {
    let columns: Vec<ColumnInfo> = stmt.columns.into_iter().enumerate().map(|(i, col)| ColumnInfo {
        name: col.name,
        dtype: col.datatype,
        offset: i,
    }).collect();

    let table_info: TableInfo = TableInfo {
        id: ctx.table_id_alloc.base,
        name: stmt.table_name,
        columns: columns,
        indices: Vec::new(),
        next_record_id: Allocator::new(1),
    };

    match ctx.db {
        None => return Err(ClientError::DatabaseNotFoundError),
        Some(ref mut db) => db.add_table(table_info.clone()),
    };

    ctx.table_id_alloc.increment();
    Ok(())
}

pub fn exec_dml(ctx: &mut Context, stmt: DML) -> Result<(), ClientError> {
    match stmt {
        DML::Insert(stmt) => exec_insert(ctx, stmt),
        DML::Select(stmt) => exec_select(ctx, stmt),
        _ => Err(ClientError::BuildExecutorError),
    }
}

pub fn exec_insert(ctx: &mut Context, stmt: InsertStmt) -> Result<(), ClientError> {
    let mut fields: Vec<Field> = Vec::new();
    let literals = stmt.values;
    for lit in literals {
        fields.push(lit.into());
    }

    match ctx.db {
        None => Err(ClientError::BuildExecutorError),
        Some(ref mut db) => {
            match db.load_tables(&[stmt.table_name]) {
                Ok(ref mut mem_tbls) => {
                    mem_tbls[0].insert(fields);
                    Ok(())
                },
                _ => Err(ClientError::BuildExecutorError),
            }
        },
    }
}

pub fn exec_select(ctx: &mut Context, stmt: SelectStmt) -> Result<(), ClientError> {
    println!("{:?}", stmt);
    match ctx.db {
        None => Err(ClientError::BuildExecutorError),
        Some(ref mut db) => {
            match stmt.source.tables.len() {
                1 => {
                    let tbl_names: &[String] = stmt.source.tables.as_slice();
                    let mut mem_tbls: Vec<MemoryTable> = try!(db.clone().load_tables(tbl_names));
                    let mut mem_tbl_infos: Vec<TableInfo> = try!(db.clone().table_infos_from_str(tbl_names));

                    let mut scan_exec: MemoryTableScanExec = MemoryTableScanExec::new(&mut mem_tbls[0], mem_tbl_infos[0].clone(), vec![Range::new(0, 10)]);

                    let mut conditions: Vec<Box<Selector>> = Vec::new();
                    match stmt.condition {
                        None => {},
                        Some(condition) => {
                            conditions = execute_where(condition, false);
                        },
                    }

                    let mut selection_exec: SelectionExec<MemoryTableScanExec> = SelectionExec::new(&mut scan_exec, conditions);
                    let mut proj_exec: ProjectionExec<SelectionExec<MemoryTableScanExec>> = ProjectionExec::new(&mut selection_exec, stmt.targets);

                    loop {
                        match proj_exec.next() {
                            None => break,
                            Some(tuple) => tuple.print(),
                        };
                    }
                    println!("Scaned\n");
                    Ok(())
                },

                2 => {
                    let mut db4left = db.clone();
                    let left_tbl_name: String = stmt.source.tables[0].clone();
                    let left_tbl_info: TableInfo = try!(db4left.clone().table_info_from_str(&left_tbl_name));
                    let mut left_mem_tbl: MemoryTable = try!(db4left.load_table(left_tbl_name));
                    let mut left_tbl_scan: MemoryTableScanExec = MemoryTableScanExec::new(&mut left_mem_tbl, left_tbl_info.clone(), vec![Range::new(0, 10)]);

                    let mut db4rht = db.clone();
                    let rht_tbl_name: String = stmt.source.tables[1].clone();
                    let rht_tbl_info: TableInfo = try!(db4rht.clone().table_info_from_str(&rht_tbl_name));
                    let mut rht_mem_tbl: MemoryTable = try!(db4rht.clone().load_table(rht_tbl_name));
                    let mut rht_tbl_scan: MemoryTableScanExec = MemoryTableScanExec::new(&mut rht_mem_tbl, rht_tbl_info.clone(), vec![Range::new(0, 10)]);


                    let mut conditions: Vec<Box<Selector>> = Vec::new();
                    match stmt.condition.clone() {
                        None => {},
                        Some(condition) => {
                            conditions = execute_where(condition, false);
                        },
                    }

                    let mut join_exec = NestedLoopJoinExec::new(&mut left_tbl_scan, &mut rht_tbl_scan, stmt.source.condition.clone());
                    let mut selection_exec = SelectionExec::new(&mut join_exec, conditions);
                    let mut proj_exec = ProjectionExec::new(&mut selection_exec, stmt.targets);

                    loop {
                        match proj_exec.next() {
                            None => break,
                            Some(tuple) => tuple.print(),
                        };
                    }
                    println!("Scaned\n");
                    Ok(())
                },

                _ => Err(ClientError::BuildExecutorError),
            }
        }
    }
}

pub fn execute_where(condition: Conditions, is_or: bool) -> Vec<Box<Selector>> {
    match condition {
        Conditions::And(c1, c2) => {
            let mut selectors1: Vec<Box<Selector>> = execute_where(*c1, false);
            let mut selectors2: Vec<Box<Selector>> = execute_where(*c2, false);
            selectors1.append(&mut selectors2);
            selectors1
        },

        Conditions::Or(c1, c2) => {
            let mut selectors1: Vec<Box<Selector>> = execute_where(*c1, true);
            let mut selectors2: Vec<Box<Selector>> = execute_where(*c2, true);
            selectors1.append(&mut selectors2);
            selectors1
        },

        Conditions::Leaf(condition) => {
            match condition.op {
                Operator::Equ => {
                    if is_or {
                        match condition.right {
                            Comparable::Lit(l) => vec![Equal::new(condition.left, None, Some(l.into()))],
                            Comparable::Target(t) => vec![Equal::new(condition.left, Some(t), None)],
                        }
                    } else {
                        match condition.right {
                            Comparable::Lit(l) => vec![NotEqual::new(condition.left, None, Some(l.into()))],
                            Comparable::Target(t) => vec![NotEqual::new(condition.left, Some(t), None)],
                        }
                    }
                },

                Operator::NEqu => {
                    if is_or {
                        match condition.right {
                            Comparable::Lit(l) => vec![NotEqual::new(condition.left, None, Some(l.into()))],
                            Comparable::Target(t) => vec![NotEqual::new(condition.left, Some(t), None)],
                        }
                    } else {
                        match condition.right {
                            Comparable::Lit(l) => vec![Equal::new(condition.left, None, Some(l.into()))],
                            Comparable::Target(t) => vec![Equal::new(condition.left, Some(t), None)],
                        }
                    }
                },

                Operator::GT => {
                    if is_or {
                        match condition.right {
                            Comparable::Lit(l) => vec![GT::new(condition.left, None, Some(l.into()))],
                            Comparable::Target(t) => vec![GT::new(condition.left, Some(t), None)],
                        }
                    } else {
                        match condition.right {
                            Comparable::Lit(l) => vec![LE::new(condition.left, None, Some(l.into()))],
                            Comparable::Target(t) => vec![LE::new(condition.left, Some(t), None)],
                        }
                    }
                },

                Operator::LT => {
                    if is_or {
                        match condition.right {
                            Comparable::Lit(l) => vec![LT::new(condition.left, None, Some(l.into()))],
                            Comparable::Target(t) => vec![LT::new(condition.left, Some(t), None)],
                        }
                    } else {
                        match condition.right {
                            Comparable::Lit(l) => vec![GE::new(condition.left, None, Some(l.into()))],
                            Comparable::Target(t) => vec![GE::new(condition.left, Some(t), None)],
                        } 
                    }
                },

                Operator::GE => {
                    if is_or {
                        match condition.right {
                            Comparable::Lit(l) => vec![GE::new(condition.left, None, Some(l.into()))],
                            Comparable::Target(t) => vec![GE::new(condition.left, Some(t), None)],
                        }
                    } else {
                        match condition.right {
                            Comparable::Lit(l) => vec![LT::new(condition.left, None, Some(l.into()))],
                            Comparable::Target(t) => vec![LT::new(condition.left, Some(t), None)],
                        }
                    }
                },

                Operator::LE => {
                    if is_or {
                        match condition.right {
                            Comparable::Lit(l) => vec![LE::new(condition.left, None, Some(l.into()))],
                            Comparable::Target(t) => vec![LE::new(condition.left, Some(t), None)],
                        }
                    } else {
                        match condition.right {
                            Comparable::Lit(l) => vec![GT::new(condition.left, None, Some(l.into()))],
                            Comparable::Target(t) => vec![GT::new(condition.left, Some(t), None)],
                        }   
                    }
                },
            }
        },
    }
}

#[derive(Debug, PartialEq)]
pub enum ClientError {
    ParseError(ParseError),
    DatabaseError(DatabaseError),
    TableInfoError(TableInfoError),
    BuildExecutorError,
    DatabaseNotFoundError,
}

impl From<ParseError> for ClientError {
    fn from(err: ParseError) -> ClientError {
        ClientError::ParseError(err)
    }
}

impl From<DatabaseError> for ClientError {
    fn from(err: DatabaseError) -> ClientError {
        ClientError::DatabaseError(err)
    }
}

impl From<TableInfoError> for ClientError {
    fn from(err: TableInfoError) -> ClientError {
        ClientError::TableInfoError(err)
    }
}

