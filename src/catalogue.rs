/**
 * Defines the CatalogueInfo object that represents DAV objects'
 * meta data.
 * 
 * (c) 2024 Andreas Feldner
 */

use dateparser::DateTimeUtc;
use minidom::Element;
use url::Url;

#[derive(Debug)]
pub struct CatalogueInfo {
    pub url: Url,
    pub name: String,
    pub size: Option<u64>,
    pub date: Option<DateTimeUtc>,
    pub file_type: Option<String>
}

macro_rules! extract_property {
    ($target:expr, $element_name:expr, $namespace_name:expr, $prop:expr) => {
        $target = match $prop.get_child($element_name, $namespace_name) {
            Some(size_node) => match size_node.text().as_str().parse() {
                Ok(size) => Some(size),
                Err(_) => None
            },
            None => None
        };
    }
}

impl CatalogueInfo {
    pub fn new(base: &Url, response: &Element) -> CatalogueInfo {
        let mut info = CatalogueInfo {url: base.to_owned(), name: String::from(""), size: None, date: None, file_type: None};
        info.name = match response.get_child("href", "DAV:") {
            Some(href_child) => href_child.text(),
            None => String::from(".")
        };
        if let Ok(joined_url) = base.join(&info.name) {
            info.url = joined_url;
        }
        if let Some(propstat) = response.get_child("propstat", "DAV:") {
            if let Some(prop) = propstat.get_child("prop", "DAV:") {
                extract_property!(info.size, "getcontentlength", "DAV:", prop);
                extract_property!(info.date, "getlastmodified", "DAV:", prop);
                extract_property!(info.file_type, "getcontenttype", "DAV:", prop);
            }
        }
        info
    }
}