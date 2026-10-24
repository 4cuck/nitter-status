// SPDX-License-Identifier: AGPL-3.0-only
use regex::{Regex, RegexBuilder};
use scraper::{Html, Selector};
use thiserror::Error;

use crate::instance_parser::EXPECT_CSS_SELCTOR;

pub type Result<T> = std::result::Result<T, AboutParseError>;

#[derive(Error, Debug)]
pub enum AboutParseError {
    #[error("No p containing a version found!")]
    NoAboutElement,
    #[error("No a element found!")]
    NoCommitLinkFound,
    #[error("Missing test! Found '{0}'")]
    InvalidCommitFormat(String),
    #[error("No valid href!")]
    NoValidHref,
}

pub(crate) struct AboutParser {
    selector_p: Selector,
    selector_a: Selector,
    regex: Regex,
    teapot_version: Regex,
}

pub struct AboutParsed {
    pub version_name: String,
    pub url: String,
}

impl AboutParser {
    /// Returns the text of the version <a> element of nitters about site
    pub fn parse_about_version(&self, html: &str) -> Result<AboutParsed> {
        let fragment = Html::parse_fragment(html);
        if let Some(parsed) = self.parse_teapot(&fragment) {
            return Ok(parsed);
        }
        if let Some(parsed) = self.parse_original_teapot(&fragment) {
            return Ok(parsed);
        }
        // get all <p> elements
        let p_elem = fragment
            .select(&self.selector_p)
            .find(|t| t.text().any(|text| text.contains("Version")))
            .ok_or(AboutParseError::NoAboutElement)?;

        let mut a_elems = p_elem.select(&self.selector_a);
        let link = a_elems.next().ok_or(AboutParseError::NoCommitLinkFound)?;
        let url = link
            .value()
            .attr("href")
            .map(|v| v.trim().to_owned())
            .ok_or(AboutParseError::NoValidHref)?;
        let link_text = link.text().fold(String::new(), |mut acc, text| {
            acc.push_str(text);
            acc
        });
        if !self.regex.is_match(&link_text) {
            return Err(AboutParseError::InvalidCommitFormat(link_text));
        }
        Ok(AboutParsed {
            url,
            version_name: link_text,
        })
    }

    /// teapot and teapawt about pages say "This site runs teapot" (or teapawt)
    /// instead of publishing a nitter commit.
    fn parse_teapot(&self, fragment: &Html) -> Option<AboutParsed> {
        let paragraph = fragment.select(&self.selector_p).find(|element| {
            element
                .text()
                .any(|text| text.to_ascii_lowercase().contains("this site runs"))
        })?;
        let link = paragraph.select(&self.selector_a).next()?;
        let name = link
            .text()
            .fold(String::new(), |mut acc, text| {
                acc.push_str(text);
                acc
            })
            .trim()
            .to_ascii_lowercase();
        if name != "teapawt" && name != "teapot" {
            return None;
        }
        let url = link.value().attr("href")?.trim().to_owned();
        let version_name = fragment
            .select(&self.selector_p)
            .find_map(|element| {
                let text = element.text().collect::<String>();
                self.teapot_version
                    .captures(&text)
                    .and_then(|caps| caps.get(1).map(|m| m.as_str().to_owned()))
            })
            .map(|version| format!("{name} {version}"))
            .unwrap_or(name);
        Some(AboutParsed { version_name, url })
    }

    /// Original teapot about pages publish "Version: teapot 0.1.0" and a GitHub
    /// link, without the "this site runs" sentence teapawt uses.
    fn parse_original_teapot(&self, fragment: &Html) -> Option<AboutParsed> {
        let version_name = fragment.select(&self.selector_p).find_map(|element| {
            let text = element.text().collect::<String>();
            let number = self
                .teapot_version
                .captures(&text)
                .and_then(|caps| caps.get(1).map(|m| m.as_str().to_owned()))?;
            let lower = text.to_ascii_lowercase();
            let name = if lower.contains("teapawt") {
                "teapawt"
            } else if lower.contains("teapot") {
                "teapot"
            } else {
                return None;
            };
            Some(format!("{name} {number}"))
        })?;
        let url = fragment
            .select(&self.selector_a)
            .find_map(|link| {
                let href = link.value().attr("href")?.trim();
                let lower = href.to_ascii_lowercase();
                if lower.contains("github.com") && lower.contains("teapot") {
                    Some(href.to_owned())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "https://github.com/amaanq/teapot".to_owned());
        Some(AboutParsed { version_name, url })
    }

    pub fn new() -> Self {
        let mut builder = RegexBuilder::new(r#"^((\d+\.\d+\.\d+)|[a-zA-Z0-9]{7,})"#);
        builder.case_insensitive(true);
        Self {
            selector_p: Selector::parse("p").expect(EXPECT_CSS_SELCTOR),
            selector_a: Selector::parse("a").expect(EXPECT_CSS_SELCTOR),
            regex: builder.build().expect("failed to generate regex"),
            teapot_version: RegexBuilder::new(r#"(?i)version:\s*(?:teapawt|teapot)\s+v?(\d+\.\d+\.\d+)"#)
                .case_insensitive(true)
                .build()
                .expect("failed to generate teapot version regex"),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn parse() {
        let html = include_str!("../test_data/about.html");
        let parser = AboutParser::new();
        let res = parser.parse_about_version(html).unwrap();
        assert_eq!(&res.version_name, "2023.07.22-72d8f35");
        assert_eq!(
            res.url,
            String::from("https://github.com/zedeus/nitter/commit/72d8f35")
        );
    }

    #[test]
    fn parse_teapawt() {
        let html = r#"<p>This site runs <a href="https://github.com/4cuck/teapawt">teapawt</a>, a privacy-focused Twitter/X frontend.</p><p>Version: teapawt 0.19.0</p>"#;
        let parser = AboutParser::new();
        let res = parser.parse_about_version(html).unwrap();
        assert_eq!(res.version_name, "teapawt 0.19.0");
        assert_eq!(res.url, "https://github.com/4cuck/teapawt");
    }

    #[test]
    fn parse_teapot() {
        let html = r#"<p>This site runs <a href="https://github.com/example/teapot">teapot</a>.</p>"#;
        let parser = AboutParser::new();
        let res = parser.parse_about_version(html).unwrap();
        assert_eq!(res.version_name, "teapot");
        assert_eq!(res.url, "https://github.com/example/teapot");
    }

    #[test]
    fn parse_original_teapot() {
        let html = r#"<p>The source is available on GitHub at <a href="https://github.com/amaanq/teapot">https://github.com/amaanq/teapot</a></p><p>Version: teapot 0.1.0</p>"#;
        let parser = AboutParser::new();
        let res = parser.parse_about_version(html).unwrap();
        assert_eq!(res.version_name, "teapot 0.1.0");
        assert_eq!(res.url, "https://github.com/amaanq/teapot");
    }
}
