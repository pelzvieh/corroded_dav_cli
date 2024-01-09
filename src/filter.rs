/**
 * Defines the FilterCriteria object that can be used to
 * filter DAV objects.
 * 
 * (c) 2024 Andreas Feldner
 */

use chrono::DateTime;
use chrono::offset::Utc;
use chrono::ParseError;
use std::num::ParseIntError;
use derive_more::{Display, From};
use dateparser::DateTimeUtc;
use crate::catalogue::CatalogueInfo;


#[derive(Debug, Display, From)]
pub enum FilterCriteriaError {
    ParseError(String)
}

impl std::error::Error for FilterCriteriaError {}

impl From<ParseError> for FilterCriteriaError {
    fn from(chrono_err: ParseError) -> Self{
        FilterCriteriaError::ParseError(chrono_err.to_string())
    }
}

impl From<ParseIntError> for FilterCriteriaError {
    fn from(parseerr: ParseIntError) -> Self{
        FilterCriteriaError::ParseError(parseerr.to_string())
    }
}


pub struct FilterCriteria {
    file_type: Option<String>,
    min_size: Option<u64>,
    max_size: Option<u64>,
    earliest_modification: Option<DateTime<Utc>>,
    latest_modification: Option<DateTime<Utc>>
}

macro_rules! parse_filter_desc {
    ($desc:expr, $resulttype:ty) => {
        if $desc == "*" {
            None
        } else {
            Some($desc.parse::<$resulttype>()?)
        }
    }
}

impl FilterCriteria {
    pub fn new(file_type_desc: &str, 
           min_size_desc: &str, 
           max_size_desc: &str, 
           earliest_modification_desc: &str,
           latest_modification_desc: &str) -> Result<Self, FilterCriteriaError> {
        Ok(Self {
            file_type: if file_type_desc=="*" {None} else {Some(file_type_desc.to_string())},
            min_size: parse_filter_desc! (min_size_desc, u64),
            max_size: parse_filter_desc! (max_size_desc, u64),
            earliest_modification: parse_filter_desc! (earliest_modification_desc, DateTime<Utc>),
            latest_modification: parse_filter_desc! (latest_modification_desc, DateTime<Utc>)
        })
    }
    
    pub fn match_all() -> Self {
        Self {file_type: None, min_size: None, max_size: None, earliest_modification: None, latest_modification: None}
    }
    
    pub fn matches(&self, attrs: &CatalogueInfo) -> bool {
        if let Some(size) = attrs.size {
            if size > self.max_size.unwrap_or(u64::max_value()) {
                return false;
            }
            if size < self.min_size.unwrap_or(0) {
                return false;
            }
        }
        if let Some(DateTimeUtc(creation_date)) = attrs.date {
            if creation_date < self.earliest_modification.unwrap_or(creation_date) {
                return false;
            }
            if creation_date > self.latest_modification.unwrap_or(creation_date) {
                return false;
            }
        }
        if let Some(regex) = self.file_type.as_ref() {
            if let Some(file_type) = attrs.file_type.as_ref() {
                return regex.find(file_type).is_some();
            } else {
                // there's a filter on file_type, but this entry doesn't have a type
                return false;
            }
        }
        true
    }

}
