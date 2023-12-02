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

struct DavCmdController {
    dav_client: Client,
    base_url: Option<Url>,
    running: bool
}

macro_rules! extract_property {
        ($target:ident,$element_name:expr, $namespace_name:expr,$prop:expr) => {
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

    fn _print_dav_response (&self, response: &Element) {
        if !response.is("response", "DAV:") {
            return;
        }
        let name: String = match response.get_child("href", "DAV:") {
            Some(href_child) => href_child.text(),
            None => String::from(".")
        };
        if let Some(propstat) = response.get_child("propstat", "DAV:") {
            if let Some(prop) = propstat.get_child("prop", "DAV:") {
                let size: Option<i64>;
                extract_property!(size, "getcontentlength", "DAV:", prop);
                let date: Option<DateTimeUtc>;
                extract_property!(date, "getlastmodified", "DAV:", prop);
                println!("{}\t{}\t{}", name, 
                    match size {Some(wert) => wert.to_string(), None => "---".to_string()}, 
                    match date {Some(DateTimeUtc(wert)) => wert.to_rfc3339(), None => "---".to_string()});
            }
        }
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
            
            //root.write_to(&mut std::io::stdout())?;
            for content in root.children() {
                if content.is("response", "DAV:") {
                    self._print_dav_response (content);
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
