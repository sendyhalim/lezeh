/// let m: HashMap<i32, i32> = {
///  1 => 2,
///  3 => 4
/// };
#[allow(unused_macros)]
macro_rules! hashmap_literal {
  ($($key:expr => $value:expr),* $(,)?) => {{
    use std::collections::HashMap;

    HashMap::from([
      $(($key, $value),)*
    ])
  }}
}

#[allow(unused_imports)]
pub(crate) use hashmap_literal;
