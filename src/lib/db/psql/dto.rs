use std::borrow::Cow;
use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::Hash;
use std::rc::Rc;

use postgres::Row;

type AnyString<'a> = Cow<'a, str>;

#[derive(PartialEq, Hash, Eq, Debug, Clone)]
pub struct PsqlTableColumn<'a> {
  pub name: AnyString<'a>,
  pub data_type: AnyString<'a>,
}

impl<'a> PsqlTableColumn<'a> {
  pub fn new<S>(name: S, data_type: S) -> PsqlTableColumn<'a>
  where
    S: Into<AnyString<'a>>,
  {
    return PsqlTableColumn {
      name: name.into(),
      data_type: data_type.into(),
    };
  }
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct PsqlForeignKey<'a> {
  pub name: AnyString<'a>,
  pub column: PsqlTableColumn<'a>,
  pub foreign_table_schema: AnyString<'a>,
  pub foreign_table_name: AnyString<'a>,
}

impl<'a> PsqlForeignKey<'a> {
  pub fn new<S>(
    name: S,
    column: PsqlTableColumn<'a>,
    foreign_table_schema: S,
    foreign_table_name: S,
  ) -> PsqlForeignKey<'a>
  where
    S: Into<AnyString<'a>>,
  {
    return PsqlForeignKey {
      name: name.into(),
      column: column,
      foreign_table_schema: foreign_table_schema.into(),
      foreign_table_name: foreign_table_name.into(),
    };
  }
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct PsqlTable<'a> {
  pub schema: AnyString<'a>,
  pub name: AnyString<'a>,
  pub primary_column: PsqlTableColumn<'a>,
  pub columns: HashSet<PsqlTableColumn<'a>>,
  pub referenced_fk_by_constraint_name: HashMap<String, PsqlForeignKey<'a>>,
  pub referencing_fk_by_constraint_name: HashMap<String, PsqlForeignKey<'a>>,
}

impl<'a> PsqlTable<'a> {
  pub fn new<S>(
    schema: S,
    name: S,
    primary_column: PsqlTableColumn<'a>,
    columns: HashSet<PsqlTableColumn<'a>>,
    referenced_fk_by_constraint_name: HashMap<String, PsqlForeignKey<'a>>,
    referencing_fk_by_constraint_name: HashMap<String, PsqlForeignKey<'a>>,
  ) -> PsqlTable<'a>
  where
    S: Into<AnyString<'a>>,
  {
    return PsqlTable {
      schema: schema.into(),
      name: name.into(),
      primary_column,
      columns,
      referenced_fk_by_constraint_name,
      referencing_fk_by_constraint_name,
    };
  }
}

#[derive(Debug, Clone)]
pub struct PsqlTableRows<'a> {
  pub table: PsqlTable<'a>,
  pub rows: Vec<Rc<Row>>,
}

impl<'a> PartialEq for PsqlTableRows<'a> {
  fn eq(&self, other: &Self) -> bool {
    return self.table == other.table;
  }
}

impl<'a> Eq for PsqlTableRows<'a> {}

impl<'a> Hash for PsqlTableRows<'a> {
  fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
    return self.table.name.hash(state);
  }
}
