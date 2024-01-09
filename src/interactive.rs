/**
 * Defines the DavCmdController object that can be used to
 * handle interactive user sessions with DAV servers.
 * 
 * (c) 2024 Andreas Feldner
 */

use rustydav::prelude::Error as DavError;
use std::str::SplitWhitespace;
use std::io::{Error as IoError, ErrorKind};
use url::{ParseError as ParseUrlError, Url};
use rustyline::error::ReadlineError;
use derive_more::Display;
use dateparser::DateTimeUtc;
use netrc::Netrc;
use std::path::PathBuf;
use crate::filter::{FilterCriteria,FilterCriteriaError};
use crate::catalogue::CatalogueInfo;
use crate::davctrl::{DavController, DavCtrlError};

#[derive(Debug, Display)]
pub enum CmdControllerError {
    IllegalUse(String),
    UnknownCommand(String),
    IoError(IoError),
    DavError(DavError),
}

impl std::error::Error for CmdControllerError {}

impl From<FilterCriteriaError> for CmdControllerError {
    fn from(e: FilterCriteriaError) -> Self{
        Self::IllegalUse(format!("Unparseable filter criteria: {e}"))
    }
}

impl From<DavCtrlError> for CmdControllerError {
    fn from(e: DavCtrlError) -> Self{
        match e {
            DavCtrlError::Local(e_io) => Self::IoError(e_io),
            DavCtrlError::Dav(e_dav) => Self::DavError(e_dav),
            DavCtrlError::InvalidSource(e_inval) => Self::IllegalUse(format!("Invalid source: {e_inval}")),
            DavCtrlError::InvalidDestination(e_invald) => Self::IllegalUse(format!("Invalid destination: {e_invald}"))
        }
    }
}

impl From<ParseUrlError> for CmdControllerError {
    fn from(e: ParseUrlError) -> Self {
        Self::IllegalUse(format!("url::ParseError: {e}"))
    }
}

impl From<IoError> for CmdControllerError {
    fn from(e: IoError) -> Self {Self::IoError(e)}
}

impl From<ReadlineError> for CmdControllerError {
    fn from(e: ReadlineError) -> Self {Self::IoError(IoError::new(ErrorKind::Other, e))}
}

pub struct DavCmdController {
    dav_ctrl: DavController,
    base_url: Option<Url>,
    running: bool
}

impl DavCmdController {
    pub fn new(rc: Netrc) -> DavCmdController{
        DavCmdController{
            dav_ctrl: DavController::new(rc),
            base_url: None,
            running: true
        }
    }
    
    fn cmd_login(&mut self, mut args: SplitWhitespace) -> Result<bool, CmdControllerError> {
        let username = Self::_next_arg(&mut args)?.to_string();
        let password = Self::_next_arg(&mut args)?.to_string();
        self.dav_ctrl.set_default_credentials(username, password);
        Ok(true)
    }
    
    fn cmd_connect(&mut self, mut args: SplitWhitespace) -> Result<bool, CmdControllerError> {
        let url_str = Self::_next_arg(&mut args)?;
        let url = Url::parse(url_str)?;
        self.base_url = Some(url);
        Ok(true)
    }
    
    fn cmd_put(&self, mut args: SplitWhitespace) -> Result<bool, CmdControllerError> {
        let file_str = Self::_next_arg(&mut args)?.to_owned();
        let path_str = Self::_next_arg(&mut args)?;
        let (_, target_url) = self._url_for_path_string(&path_str)?;
        let path = PathBuf::from(file_str);
        let mut result_vec = self.dav_ctrl.put(&vec!(&path), &target_url);
        if result_vec.len() != 1 {
            return Err(CmdControllerError::IoError(IoError::new(
                ErrorKind::InvalidData, format!("Unexpected result vector length {}", result_vec.len()))))
        }
        // unwrap is safe here because we checked the vector' length
        let result = result_vec.pop().unwrap()?.status();
        println!("Put {} to {target_url}: {result}", path.display());
        Ok(true)
    }
    
    fn cmd_get(&self, mut args: SplitWhitespace) -> Result<bool, CmdControllerError> {
        let path_str = Self::_next_arg(&mut args)?.to_owned();
        let file_path = PathBuf::from(Self::_next_arg(&mut args)?);
        let (_, source_url) = self._url_for_path_string(&path_str)?;
        let mut result_vec = self.dav_ctrl.get(&vec!(&source_url), &file_path);
        if result_vec.len() != 1 {
            return Err(CmdControllerError::IoError(IoError::from(ErrorKind::InvalidData)))
        }
        let result = result_vec.pop().unwrap()?.status();
        let file_path_printable = file_path.display();
        println!("Got {source_url} to {file_path_printable}: {result}");
        Ok(true)
    }

    fn _print_attrs(attrs: &CatalogueInfo) {
        println!("{}\t{}\t{}\t{}", attrs.url, 
            match attrs.size {Some(wert) => wert.to_string(), None => "---".to_string()}, 
            match attrs.date {Some(DateTimeUtc(wert)) => wert.to_rfc3339(), None => "---".to_string()},
            match attrs.file_type.as_ref() {Some(wert) => wert.clone(), None => "---".to_string()});
    }
    
    fn _url_for_path_string(&self, path_str: &str) -> Result<(&Url, Url), CmdControllerError> {
        let base_url = (self.base_url.as_ref().ok_or(
                CmdControllerError::IllegalUse("Not initialised, you need to call connect before put".to_string())
            ))?;
        let target_url = base_url.join(path_str)?;
        Ok((base_url, target_url))
    }

    fn cmd_ls(&self, mut args: SplitWhitespace) -> Result<bool, CmdControllerError> {
        let path_str = Self::_next_arg(&mut args)?;
        let (_, target_url) = self._url_for_path_string(&path_str)?;
        let element_catalogue = self.dav_ctrl.ls(&target_url, &FilterCriteria::match_all())?;
        for attrs in element_catalogue {
            Self::_print_attrs(&attrs);
        }
        println!();
        Ok(true)
    }
    
    fn cmd_ls_by_criteria(&self, mut args: SplitWhitespace) -> Result<bool, CmdControllerError> {
        let path_str = Self::_next_arg(&mut args)?.to_string();
        let filter = FilterCriteria::new(
                Self::_next_arg(&mut args)?.to_owned().as_str(),
                Self::_next_arg(&mut args)?.to_owned().as_str(),
                Self::_next_arg(&mut args)?.to_owned().as_str(),
                Self::_next_arg(&mut args)?.to_owned().as_str(),
                Self::_next_arg(&mut args)?
        )?;
        let (_, target_url) = self._url_for_path_string(&path_str)?;
        let element_catalogue = self.dav_ctrl.ls(&target_url, &filter)?;
        for attrs in element_catalogue {
            Self::_print_attrs(&attrs);
        }
        println!();
        Ok(true)
    }
    
    fn cmd_delete(&self, mut args: SplitWhitespace) -> Result<bool, CmdControllerError> {
        let path_str = Self::_next_arg(&mut args)?.to_string();
        let (_, target_url) = self._url_for_path_string(&path_str)?;
        let result = self.dav_ctrl.delete(&target_url);
        match result {
            Err(error) => {
                eprintln!("Failed to delete {target_url}: {error}");
                Err(CmdControllerError::from(error))
            },
            Ok(response) => {
                let status = response.status();
                println!("Successfully deleted {target_url}: {status}");
                Ok(true)
            } 
        }
    }
    
    fn cmd_delete_by_criteria(&self, mut args: SplitWhitespace) -> Result<bool, CmdControllerError> {
        let path_str = Self::_next_arg(&mut args)?.to_string();
        let filter = FilterCriteria::new(
                Self::_next_arg(&mut args)?.to_owned().as_str(),
                Self::_next_arg(&mut args)?.to_owned().as_str(),
                Self::_next_arg(&mut args)?.to_owned().as_str(),
                Self::_next_arg(&mut args)?.to_owned().as_str(),
                Self::_next_arg(&mut args)?
        )?;
        let (_, target_url) = self._url_for_path_string(&path_str)?;
        let element_catalogue = self.dav_ctrl.ls(&target_url, &filter)?;
        let number = element_catalogue.len();
        println!("About to delete {number} entries");
        let mut last_error: Option<DavCtrlError> = None;
        for element in element_catalogue {
            let url = element.url;
            print!("- {url} ... ");
            match self.dav_ctrl.delete(&url) {
                Ok(_)  => { println!("Done");},
                Err(e) => {
                    println!("Error {e}"); 
                    last_error = Some(e); 
                }
            };
        }
        println!();
        match last_error {
            None => Ok(true),
            Some(error) => Err(CmdControllerError::from(error))
        }
    }

    fn cmd_quit(&mut self, _args: SplitWhitespace) -> Result<bool, CmdControllerError> {
        self.running = false;
        Ok(true)
    }

    fn _next_arg<'a>(args: &'a mut SplitWhitespace) -> Result<&'a str, CmdControllerError> {
        args.next().
            ok_or_else(|| CmdControllerError::IllegalUse("required argument missing".to_string()))
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
            Some("delete") => self.cmd_delete(words),
            Some("delete-by-criteria") => self.cmd_delete_by_criteria(words),
            Some("quit") => self.cmd_quit(words),
            Some(_unknown_cmd) => Err(CmdControllerError::UnknownCommand("unknown command".to_string()))
        };
        
        let success = match success_result {
            Err(error) => {
                eprintln!("Command failed with error {error}");
                false
            },
            Ok(flag) => flag
        };
    
        if !success {
            eprintln!("Your commandline '{line}' FAILED");
        } else {
            println!("OK");
        }
    }
    
    pub fn run(&mut self, rl: &mut rustyline::DefaultEditor) -> Result<(), CmdControllerError> {
        while self.running {
            let prompt_path = match &self.base_url {
                Some(url) => url.as_str(),
                None => "?"
            };
            let line = rl.readline(format!("{prompt_path}> ").as_str())?;
            self.handle_command(&line);
        }
        Ok(())
    }
}
