use ScanIterator;
use Selector;
use meta::table_info::{TableInfo, TableInfoError};
use meta::column_info::ColumnInfo;
use columns::column::Column;
use tables::tuple::Tuple;
use allocators::allocator::Allocator;

use parser::statement::*;
use executors::selector::*;

//#[derive(Debug)]
pub struct NestedLoopJoinExec<'n> {
    pub cursor: usize,
    pub outer_columns: Vec<Column>,
    pub inner_columns: Vec<Column>,
    pub next_tuple: Box<FnMut() -> Option<Tuple> + 'n>,
    pub selectors: Vec<Box<Selector>>,
    pub meta: TableInfo,
}

impl<'n> NestedLoopJoinExec<'n> {
    pub fn new<T1: ScanIterator, T2: ScanIterator>(outer_table: &'n mut T1, inner_table: &'n mut T2, condition: Option<Condition>) -> NestedLoopJoinExec<'n> {
        let outer_column_length: usize = outer_table.get_meta().columns.len();
        let mut column_infos: Vec<ColumnInfo> = outer_table.get_meta().columns;
        for (i, column) in inner_table.get_meta().columns.iter().enumerate() {
            let col: ColumnInfo = column.clone();
            column_infos.push(ColumnInfo {
                name: col.name,
                dtype: col.dtype,
                offset: outer_column_length + i,
            });
        }
        let meta: TableInfo = TableInfo {
            id: 0,
            name: "".to_owned(),
            columns: column_infos,
            indices: Vec::new(),
            next_record_id: Allocator::new(1),
        };

        let selectors: Vec<Box<Selector>> = match filterize(meta.clone(), condition) {
            Ok(f) => f,
            Err(_e) => Vec::new(),
        };

        NestedLoopJoinExec {
            cursor: 0,
            outer_columns: outer_table.get_columns(),
            inner_columns: inner_table.get_columns(),
            next_tuple: next_tuple(outer_table, inner_table),
            selectors: selectors,
            meta: meta,
        }
    }
}

impl<'n> ScanIterator for NestedLoopJoinExec<'n> {
    fn get_meta(&self) -> TableInfo {
        self.meta.clone()
    }

    fn get_columns(&self) -> Vec<Column> {
        let outer_length: usize = self.outer_columns.len();
        let mut outer_columns = self.outer_columns.clone();
        let mut inner_columns: Vec<Column> = self.inner_columns.clone().into_iter().map(|c| Column {
            table_name: c.table_name,
            name: c.name,
            dtype: c.dtype,
            offset: c.offset + outer_length,
        }).collect();

        outer_columns.append(&mut inner_columns);
        outer_columns
    }
}

impl<'n> Iterator for NestedLoopJoinExec<'n> {
    type Item = Tuple;
    fn next(&mut self) -> Option<Tuple> {
        loop {
            match (self.next_tuple)() {
                None => return None,
                Some(tuple) => {
                    let mut passed: bool = true;
                    for ref selector in &self.selectors {
                        if !selector.is_true(&tuple, &self.get_columns()) {
                          passed = false;
                          break;
                        }
                    }

                    if passed {
                        return Some(tuple);
                    }
                },
            }
        }
    }
}

fn next_tuple<'n, T1: ScanIterator + 'n, T2: ScanIterator + 'n>(outer_table: &'n mut T1, inner_table: &'n mut T2) -> Box<FnMut() -> Option<Tuple> + 'n> {
    Box::new(move || {
        loop {
            match outer_table.next() {
                None => return None,
                Some(ref outer_tuple) => {
                    loop {
                        match inner_table.next() {
                            None => break,
                            Some(ref inner_tuple) => {
                                let joined_tuple: Tuple = outer_tuple.append(inner_tuple);
                                return Some(joined_tuple);
                            }
                        }
                    }
                }
            }
        }
    })
}

fn filterize(meta: TableInfo, condition: Option<Condition>) -> Result<Vec<Box<Selector>>, JoinExecError> {
    let mut filters: Vec<Box<Selector>> = Vec::new();
    match condition {
        None => Ok(filters),
        Some(condition) => {
            match condition.op {
                Operator::Equ => {
                    let filter = match condition.right {
                        Comparable::Lit(l) => Equal::new(condition.left, None, Some(l.into())),
                        Comparable::Word(ref s) => {
                            let right_column_info: ColumnInfo = try!(meta.column_info_from_str(s));
                            Equal::new(condition.left, Some(right_column_info.offset), None)
                        },
                    };
                    filters.push(filter);
                },

                Operator::NEqu => {
                    let filter = match condition.right {
                        Comparable::Lit(l) => NotEqual::new(condition.left, None, Some(l.into())),
                        Comparable::Word(ref s) => {
                            let right_column_info: ColumnInfo = try!(meta.column_info_from_str(s));
                            NotEqual::new(condition.left, Some(right_column_info.offset), None)
                        },
                    };
                    filters.push(filter);
                },

                Operator::GT => {
                    let filter = match condition.right {
                        Comparable::Lit(l) => GT::new(condition.left, None, Some(l.into())),
                        Comparable::Word(ref s) => {
                            let right_column_info: ColumnInfo = try!(meta.column_info_from_str(s));
                            GT::new(condition.left, Some(right_column_info.offset), None)
                        },
                    };
                    filters.push(filter);
                },

                Operator::LT => {
                    let filter = match condition.right {
                        Comparable::Lit(l) => LT::new(condition.left, None, Some(l.into())),
                        Comparable::Word(ref s) => {
                            let right_column_info: ColumnInfo = try!(meta.column_info_from_str(s));
                            LT::new(condition.left, Some(right_column_info.offset), None)
                        },
                    };
                    filters.push(filter);
                },

                Operator::GE => {
                    let filter = match condition.right {
                        Comparable::Lit(l) => GE::new(condition.left, None, Some(l.into())),
                        Comparable::Word(ref s) => {
                            let right_column_info: ColumnInfo = try!(meta.column_info_from_str(s));
                            GE::new(condition.left, Some(right_column_info.offset), None)
                        },
                    };
                    filters.push(filter);
                },

                Operator::LE => {
                    let filter = match condition.right {
                        Comparable::Lit(l) => LE::new(condition.left, None, Some(l.into())),
                        Comparable::Word(ref s) => {
                            let right_column_info: ColumnInfo = try!(meta.column_info_from_str(s));
                            LE::new(condition.left, Some(right_column_info.offset), None)
                        },
                    };
                    filters.push(filter);
                },
            }

            Ok(filters)
        },
    }
}

#[derive(Debug, PartialEq)]
pub enum JoinExecError {
    TableInfoError(TableInfoError),
    TableNotFoundError,
}

impl From<TableInfoError> for JoinExecError {
    fn from(err: TableInfoError) -> JoinExecError {
        JoinExecError::TableInfoError(err)
    }
}
