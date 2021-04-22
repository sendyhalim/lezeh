use crate::types::ResultDynError;

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
  ) -> ResultDynError<String> {
    return self
      .handlebars_client
      .render_template(template, &handlebars::to_json(json_serializible))
      .map_err(failure::err_msg);
  }
}
