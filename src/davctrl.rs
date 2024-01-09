/**
 * Defines the DavCtrl object that provides functions
 * to perform WebDAV operations.
 * 
 * It does not keep a dialgoue or session context with the
 * caller, so is quite stateless apart from configuration.
 * 
 * It is currently build around rustydav, but is in
 * conflict with its design decision to tie one
 * username/password pair to a client instance.
 * 
 * (c) 2024 Andreas Feldner
 */
use rustydav::client::Client;
use rustydav::prelude::{Response, Error as DavError};
use url::{ParseError as ParseUrlError, Url};
use std::fs::File;
use std::path::Path;
use std::io::{Error as IoError, ErrorKind, BufWriter, Write, BufReader};
use netrc::Netrc;
use derive_more::Display;
use minidom::{Element, Error as DomError};
use crate::catalogue::CatalogueInfo;
use crate::filter::FilterCriteria;

#[derive(Debug, Display)]
pub enum DavCtrlError {
    Dav(DavError),
    InvalidSource(String),
    InvalidDestination(String),
    Local(IoError)
}
impl std::error::Error for DavCtrlError {}

impl From<DavError> for DavCtrlError {
    fn from(e: DavError) -> Self {
        Self::Dav(e)
    }
}
impl From<IoError> for DavCtrlError {
    fn from(e: IoError) -> Self {
        Self::Local(e)
    }
}

impl From<ParseUrlError> for DavCtrlError {
    fn from(e: ParseUrlError) -> Self {
        Self::InvalidSource(format!("Given text '{e}' is not a valid URL"))
    }
}

impl From<DomError> for DavCtrlError {
    fn from(e: DomError) -> Self {
        Self::Local(IoError::new(ErrorKind::Other, e))
    }
}

pub struct DavController {
    netrc: Netrc
}

impl DavController {
    pub fn new (rc: Netrc) -> Self{
        Self{netrc: rc}
    }
    
    fn _find_in_netrc(&self, url_host: url::Host<&str>) -> Option<&netrc::Machine> {
        for host in &self.netrc.hosts {
            let (netrc_host, netrc_machine) = host;
            if let url::Host::Domain(hostname) = url_host {
                if hostname.eq(netrc_host) {
                    return Some(netrc_machine);
                }
            }
        }
        self.netrc.default.as_ref()
    }
    

    fn _build_client(&self, url: &Url) -> Client {
        if let Some(hostname) = url.host() {
            if let Some(machine) = self._find_in_netrc(hostname) {
                if let Some(password) = machine.password.as_ref() {
                    return Client::init(&machine.login, &password);
                }
            }
        }
        eprintln!("Warning: no username/password found for URL {url}");
        Client::init("", "")
    }
    
    fn _put_one (client: &Client, file_path: &Path, target_url: &Url) -> Result<Response, DavCtrlError> {
        if file_path.is_file() {
            let file = File::open(file_path)?;
            let response = client.put(file, target_url.as_str())?;
            Self::_ensure_response_ok(response)
        } else {
            Err(DavCtrlError::InvalidSource(format!("Not an existing file: {}", file_path.display())))
        }
    }
    
    pub fn put (&self, file_paths: &Vec<&Path>, target_base: &Url) -> Vec<Result<Response, DavCtrlError>> {
        let client = self._build_client(target_base);
        let mut retvec = Vec::new();
        for file_path in file_paths {
            if !target_base.path().ends_with('/') {
                // non-directory URL is acceptable only for uploading one file
                if file_paths.len() == 1 {
                    // in this case, do _not_ replace the last path segment with the file's name
                    retvec.push(Self::_put_one(&client, file_path, target_base));
                } else {
                    retvec.push(Err(DavCtrlError::InvalidDestination(
                        format!("Given target URL {target_base} is not a directory and cannot receive multiple files")
                    )));
                }
            } else if let Some(filename) = file_path.file_name() {
                match target_base.join(&filename.to_string_lossy()) {
                    Ok(target_url) => {
                        retvec.push(Self::_put_one(&client, file_path, &target_url));
                    },
                    Err(error) => {
                        retvec.push(Err(DavCtrlError::from(error)));
                    }
                }
            } else {
                retvec.push(
                    Err(DavCtrlError::InvalidSource(
                        format!("Source path '{}' does not end with a file name", file_path.display())))
                );
            }
        }
        retvec
    }
    
    fn _get_one(client: &Client, source: &Url, target_dir: &Path) -> Result<Response, DavCtrlError> {
        if target_dir.is_dir() {
            let filename = source.path_segments().
                    and_then(|paths| paths.last()).
                    ok_or_else(|| DavCtrlError::InvalidSource(format!("Source URL '{}' contains no filename", source)))?;
            let file = std::fs::File::create(target_dir.join(filename))?;
            let mut response = client.get(source.as_str())?;
            response = Self::_ensure_response_ok(response)?;
            let mut buffer = BufWriter::new(file);
            response.copy_to(&mut buffer)?;
            buffer.flush()?;
            Ok(response)
        } else {
            Err(DavCtrlError::InvalidDestination(format!("Destination '{}' is not a directory", target_dir.display())))
        }
    }
    
    pub fn get (&self, sources: &Vec<&Url>, target_dir: &Path) -> Vec<Result<Response, DavCtrlError>> {
        let mut retvec = Vec::new();
        for source in sources {
            let client = self._build_client(source);
            retvec.push(Self::_get_one(&client, source, target_dir));
        }
        retvec
    }
    
    fn _read_attributes_from_response (&self, base: &Url, response: &Element) -> Result<CatalogueInfo, DavCtrlError> {
        if !response.is("response", "DAV:") {
            return Err(DavCtrlError::Local(IoError::from(ErrorKind::InvalidData)));
        }

        Ok(CatalogueInfo::new(base, response))
    }
    
    pub fn ls (&self, url_to_list: &Url, filter: &FilterCriteria) -> Result<Vec<CatalogueInfo>, DavCtrlError> {
        let client = self._build_client(url_to_list);
        let mut retvec = Vec::new();
        let mut response = client.list(url_to_list.as_str(), "1")?;
        response = Self::_ensure_response_ok(response)?;
        let buf_reader = BufReader::new(response);
        let root = Element::from_reader(buf_reader)?;
        if !root.is("multistatus", "DAV:") {
            return Err(DavCtrlError::Local(IoError::from(ErrorKind::InvalidData)));
        };
        
        for content in root.children() {
            if content.is("response", "DAV:") {
                let attrs = self._read_attributes_from_response(url_to_list, content)?;
                if filter.matches(&attrs) {
                    retvec.push(attrs);
                }
            }
        }
        
        Ok(retvec)
    }
    
    pub fn delete (&self, url_to_delete: &Url) -> Result<Response, DavCtrlError> {
        let client = self._build_client(url_to_delete);
        let mut response = client.delete(url_to_delete.as_str())?;
        response = Self::_ensure_response_ok(response)?;
        Ok(response)
    }
    
    fn _ensure_response_ok(response: Response) -> Result<Response, DavCtrlError> {
        if ! response.status().is_success() {
            if let Err(dav_error) = response.error_for_status_ref() {
                Err(DavCtrlError::from(dav_error))
            } else {
                Err(DavCtrlError::from(IoError::new(ErrorKind::Other, "Error status returned without information")))
            }
        } else {
            Ok(response)
        }
    }
    
    pub(crate) fn set_default_credentials(&mut self, username: String, password: String) {
        self.netrc.default = Some(netrc::Machine { 
            login: username, 
            password: Some(password), 
            account: None, 
            port: None 
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;
    use netrc::Netrc;
    use crate::filter::FilterCriteria;
    
    const TESTSERVER_URL_STR: &str = "https://www.webdavserver.com/Usere30e1ee/";
    
    fn get_testserver_url() -> Url {
        Url::parse(TESTSERVER_URL_STR).unwrap()
    }
    
    fn get_davcontroller() -> DavController {
        let netrc = Netrc::default();
        DavController::new(netrc)
    }
    
    #[test]
    fn test_ls () {
        let listing_result = get_davcontroller().ls(&get_testserver_url(), &FilterCriteria::match_all());
        assert!(listing_result.is_ok());
        let listing = listing_result.unwrap();
        assert_ne!(listing.len(), 0);
        let first_entry = &listing[0];
        assert!(first_entry.date.is_some());
        assert_ne!(first_entry.name.len(), 0);
    }
}
