// use std::borrow::Cow;
use std::io::BufRead;

use anyhow::Error;

// use crate::asset::Asset;
use crate::types::ResultAnyError;

pub struct HandlebarsRenderer {
  handlebars_client: handlebars::Handlebars<'static>,
}

impl HandlebarsRenderer {
  pub fn new() -> HandlebarsRenderer {
    let mut handlebars_client = handlebars::Handlebars::new();

    // By default handlebars will escape html. For our cases we don't want to
    // escape html, most of the use case will be on CLI so it should be safe.
    handlebars_client.register_escape_fn(handlebars::no_escape);

    return HandlebarsRenderer { handlebars_client };
  }
}

impl HandlebarsRenderer {
  pub fn render(
    &self,
    template: &str,
    json_serializible: impl serde::Serialize,
  ) -> ResultAnyError<String> {
    return self
      .handlebars_client
      .render_template(template, &handlebars::to_json(json_serializible))
      .map_err(Error::new);
  }

  pub fn render_from_template_path(
    &self,
    // template_path: &str,
    template_reader: &mut impl BufRead,
    json_serializible: impl serde::Serialize,
  ) -> ResultAnyError<String> {
    let template_string = String::from_utf8(template_reader.fill_buf()?.to_vec())?;

    return self.render(&template_string, json_serializible);
  }
}
