use rustydav::client::Client;
use rustydav::prelude::{Response, Error as DavError};
use std::str::SplitWhitespace;
use std::io::{Write, BufWriter, BufReader, Error as IoError, ErrorKind};
use url::{ParseError as ParseUrlError, Url};
use rustyline;
use minidom::{Element, Error as DomError};
use derive_more::{Display, From};
use dateparser::DateTimeUtc;
use chrono::DateTime;
use chrono::offset::Utc;
use chrono::ParseError;
use std::num::ParseIntError;

#[derive(Debug, Display, From)]
pub enum CmdControllerError {
    IllegalUse(&'static str),
    UnknownCommand(String),
    IoError(IoError),
    InvalidUrl(ParseUrlError),
    DavError(DavError),
    DomError(DomError)
}

impl std::error::Error for CmdControllerError {}

impl From<ParseIntError> for CmdControllerError {
    fn from(_parseerr: ParseIntError) -> Self{
        CmdControllerError::IllegalUse("Invalid format, positive integer number required")
    }
}

impl From<ParseError> for CmdControllerError {
    fn from(_chrono_err: ParseError) -> Self{
        CmdControllerError::IllegalUse("Invalid format, date and time format required")
    }
}

struct DavCmdController {
    dav_client: Client,
    base_url: Option<Url>,
    running: bool
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

impl DavCmdController {
    fn new() -> DavCmdController{
        DavCmdController{
            dav_client: Client::init("", ""),
            base_url: None,
            running: true
        }
    }
    
    fn cmd_login(&mut self, mut args: SplitWhitespace) -> Result<bool, CmdControllerError> {
        let username = Self::_next_arg(&mut args)?;
        let password = Self::_next_arg(&mut args)?;
        self.dav_client = Client::init(username.as_str(), password.as_str());
        Ok(true)
    }
    
    fn cmd_connect(&mut self, mut args: SplitWhitespace) -> Result<bool, CmdControllerError> {
        let url_str = Self::_next_arg(&mut args)?;
        let url = Url::parse(url_str.as_str())?;
        self.dav_client.list(url.as_str(), "0")?;
        self.base_url = Some(url);
        Ok(true)
    }
    
    fn cmd_put(&self, mut args: SplitWhitespace) -> Result<bool, CmdControllerError> {
        if let Some(base_url) = &self.base_url {
            let file_str = Self::_next_arg(&mut args)?;
            let path_str = Self::_next_arg(&mut args)?;
            let target_url = base_url.join(path_str.as_str())?;
            let file = std::fs::File::open(file_str)?;
            let response = self.dav_client.put(file, target_url.as_str())?;
            Self::_ensure_response_ok(&response)?;
            Ok(true)
        } else {
            Err(CmdControllerError::IllegalUse("Not initialised, you need to call connect before put"))
        }
    }
    
    fn cmd_get(&self, mut args: SplitWhitespace) -> Result<bool, CmdControllerError> {
        if let Some(base_url) = &self.base_url {
            let path_str = Self::_next_arg(&mut args)?;
            let file_str = Self::_next_arg(&mut args)?;
            let target_url = base_url.join(path_str.as_str())?;
            let file = std::fs::File::create(file_str)?;
            let mut response = self.dav_client.get(target_url.as_str())?;
            Self::_ensure_response_ok(&response)?;
            let mut buffer = BufWriter::new(file);
            response.copy_to(&mut buffer)?;
            buffer.flush()?;
            Ok(true)
        } else {
            Err(CmdControllerError::IllegalUse("Not initialised, you need to call connect before put"))
        }
    }

    fn _read_attributes_from_response (&self, response: &Element) -> Result<CatalogueInfo, CmdControllerError> {
        if !response.is("response", "DAV:") {
            return Err(CmdControllerError::IllegalUse("Not a response"));
        }
        let mut info = CatalogueInfo {name: String::from(""), size: None, date: None};
        info.name = match response.get_child("href", "DAV:") {
            Some(href_child) => href_child.text(),
            None => String::from(".")
        };
        if let Some(propstat) = response.get_child("propstat", "DAV:") {
            if let Some(prop) = propstat.get_child("prop", "DAV:") {
                extract_property!(info.size, "getcontentlength", "DAV:", prop);
                extract_property!(info.date, "getlastmodified", "DAV:", prop);
            }
        }
        Ok(info)
    }

    fn cmd_ls(&self, mut args: SplitWhitespace) -> Result<bool, CmdControllerError> {
        if let Some(base_url) = &self.base_url {
            let path_str = Self::_next_arg(&mut args)?;
            let target_url = base_url.join(path_str.as_str())?;
            let response = self.dav_client.list(target_url.as_str(), "1")?;
            Self::_ensure_response_ok(&response)?;
            let buf_reader = BufReader::new(response);
            let root = Element::from_reader(buf_reader)?;
            if !root.is("multistatus", "DAV:") {
                return Err(CmdControllerError::IllegalUse("list command did not return multistatus"));
            };
            
            for content in root.children() {
                if content.is("response", "DAV:") {
                    let attrs = self._read_attributes_from_response(content)?;
                    println!("{}\t{}\t{}", attrs.name, 
                        match attrs.size {Some(wert) => wert.to_string(), None => "---".to_string()}, 
                        match attrs.date {Some(DateTimeUtc(wert)) => wert.to_rfc3339(), None => "---".to_string()});
                }
            }
            println!();
            Ok(true)
        } else {
            Err(CmdControllerError::IllegalUse("Not initialised, you need to call connect before put"))
        }
    }
    
    fn _apply_optional_filter(&self, attrs: &CatalogueInfo, filter: &FilterCriteria) -> bool {
        if let Some(size) = attrs.size {
            if size > filter.max_size.unwrap_or(u64::max_value()) {
                return true;
            }
            if size < filter.min_size.unwrap_or(0) {
                return true;
            }
        }
        if let Some(DateTimeUtc(creation_date)) = attrs.date {
            if creation_date < filter.earliest_modification.unwrap_or(creation_date) {
                return true;
            }
            if creation_date > filter.latest_modification.unwrap_or(creation_date) {
                return true;
            }
        }
        false
    }

    fn cmd_ls_by_criteria(&self, mut args: SplitWhitespace) -> Result<bool, CmdControllerError> {
        if let Some(base_url) = &self.base_url {
            let path_str = Self::_next_arg(&mut args)?;
            let filter = FilterCriteria::new(
                Self::_next_arg(&mut args)?,
                Self::_next_arg(&mut args)?,
                Self::_next_arg(&mut args)?,
                Self::_next_arg(&mut args)?,
                Self::_next_arg(&mut args)?
            )?;
            let target_url = base_url.join(path_str.as_str())?;
            let response = self.dav_client.list(target_url.as_str(), "1")?;
            Self::_ensure_response_ok(&response)?;
            let buf_reader = BufReader::new(response);
            let root = Element::from_reader(buf_reader)?;
            if !root.is("multistatus", "DAV:") {
                return Err(CmdControllerError::IllegalUse("list command did not return multistatus"));
            };

            for content in root.children() {
                if content.is("response", "DAV:") {
                    let attrs = self._read_attributes_from_response (content)?;
                    if !self._apply_optional_filter (&attrs, &filter) {
                        println!("{}\t{}\t{}", attrs.name, 
                            match attrs.size {Some(wert) => wert.to_string(), None => "---".to_string()}, 
                            match attrs.date {Some(DateTimeUtc(wert)) => wert.to_rfc3339(), None => "---".to_string()});
                    }
                }
            }
            println!();
            Ok(true)
        } else {
            Err(CmdControllerError::IllegalUse("Not initialised, you need to call connect before put"))
        }
    }

    fn cmd_quit(&mut self, _args: SplitWhitespace) -> Result<bool, CmdControllerError> {
        self.running = false;
        Ok(true)
    }

    fn _next_arg(args: &mut SplitWhitespace) -> Result<String, CmdControllerError> {
        if let Some(next_arg) = args.next() {
            Ok(next_arg.to_string())
        } else {
            Err(CmdControllerError::IllegalUse("required argument missing"))
        }
    }

    fn _ensure_response_ok(response: &Response) -> Result<(), CmdControllerError> {
        if ! response.status().is_success() {
            if let Err(dav_error) = response.error_for_status_ref() {
                Err(CmdControllerError::from(dav_error))
            } else {
                Err(CmdControllerError::from(IoError::new(ErrorKind::Other, "Error status returned without information")))
            }
        } else {
            Ok(())
        }
    }
    
    fn handle_command(&mut self, line: &String) {
        let mut words = line.split_whitespace();
    
        let success_result = match words.next() {
            None => return,
            Some("login") => self.cmd_login(words),
            Some("connect") => self.cmd_connect(words),
            Some("put") => self.cmd_put(words),
            Some("get") => self.cmd_get(words),
            Some("ls") => self.cmd_ls(words),
            Some("ls-by-criteria") => self.cmd_ls_by_criteria(words),
            Some("quit") => self.cmd_quit(words),
            Some(_unknown_cmd) => Err(CmdControllerError::IllegalUse("unknown command"))
        };
        
        let success = match success_result {
            Err(error) => {
                eprintln!("Command failed with error {}", error.to_string());
                false
            },
            Ok(flag) => flag
        };
    
        if !success {
            eprintln!("Your commandline '{}' FAILED", line);
        } else {
            println!("OK");
        }
    }
}
fn main() {
    let mut rl = match rustyline::DefaultEditor::new() {
        Err(_) => panic!("Unable to construct editor"),
        Ok(editor) => editor
    };
    let mut controller = DavCmdController::new();
    println!("Hello, world!");
    while controller.running {
        let readline = rl.readline(">> ");
        match readline {
            Ok(line) => controller.handle_command(&line),
            Err(_) => break,
        }
    }
    println!("See you later");
}

struct FilterCriteria {
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
    fn new(file_type_desc: String, 
           min_size_desc: String, 
           max_size_desc: String, 
           earliest_modification_desc: String,
           latest_modification_desc: String) -> Result<FilterCriteria, CmdControllerError> {
        Ok(FilterCriteria {
            file_type: if file_type_desc=="*" {None} else {Some(file_type_desc)},
            min_size: parse_filter_desc! (min_size_desc, u64),
            max_size: parse_filter_desc! (max_size_desc, u64),
            earliest_modification: parse_filter_desc! (earliest_modification_desc, DateTime<Utc>),
            latest_modification: parse_filter_desc! (latest_modification_desc, DateTime<Utc>)
        })
    }
}

struct CatalogueInfo {
    name: String,
    size: Option<u64>,
    date: Option<DateTimeUtc>
}
