use std::borrow::Cow;

use anyhow::Error;

use crate::common::asset::Asset;
use crate::common::types::ResultAnyError;

pub struct HandlebarsRenderer {
  handlebars_client: handlebars::Handlebars<'static>,
}

impl HandlebarsRenderer {
  pub fn new() -> HandlebarsRenderer {
    return HandlebarsRenderer {
      handlebars_client: handlebars::Handlebars::new(),
    };
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
    template_path: &str,
    json_serializible: impl serde::Serialize,
  ) -> ResultAnyError<String> {
    let buf: Cow<[u8]> = Asset::get(template_path).unwrap();
    let buf: &[u8] = buf.as_ref();

    let template_string: String = String::from_utf8(Vec::from(buf)).unwrap();

    return self.render(&template_string, json_serializible);
  }
}
