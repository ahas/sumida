pub struct HtmlContent<'a>(pub &'a str);

impl<'a> HtmlContent<'a> {
  pub fn to_plain_text(&self) -> String {
    use scraper::Html;

    let fragment = Html::parse_fragment(self.0);

    fragment.root_element().text().collect::<Vec<_>>().join(" ")
  }
}
