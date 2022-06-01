use crate::db::psql::dto::PsqlTableRows;
use postgres::row::Row;
use std::collections::{HashMap, HashSet};

pub struct RelationInsert {}

impl RelationInsert {
  pub fn map(rows_by_level: HashMap<i32, HashSet<PsqlTableRows>>) {}
}
