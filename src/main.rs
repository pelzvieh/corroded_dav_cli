/**
 * Main entry of corroded_dav_cli.
 * 
 * (c) 2023 Andreas Feldner
 */
mod filter;
mod catalogue;
mod davctrl;
mod interactive;

use std::io::{BufReader, Error as IoError, ErrorKind};
use rustyline::DefaultEditor;
use netrc::Netrc;
use std::env;
use std::fs::File;
use interactive::DavCmdController;

fn read_netrc() -> Result<Netrc, IoError> {
    #[allow(deprecated)]
    // honestly, I don't care where you have to place .netrc if you run this on cygwin under Windows
    let home = env::home_dir().ok_or(IoError::from(ErrorKind::NotFound))?;
    let path = home.join(".netrc");
    if path.is_file() {
        let netrc = File::open(path)?;
        Netrc::parse(BufReader::new(netrc)).map_err(|_e| IoError::from(ErrorKind::InvalidInput))
    } else {
        Err(IoError::from(ErrorKind::NotFound))
    }
}

fn main() {
    let netrc = read_netrc().unwrap_or(Netrc::default());
    
    // parse cmd line args to find out if we're going to run interactive
    //...
    // if we're interactive, run a DavCmdController with an interactive editor
    let mut readline = DefaultEditor::new().unwrap(); // nothing useful to do if editor not constructable
    let mut session_controller = DavCmdController::new(netrc);
    println!("Entering interactive session, ready for your commands");
    let interactive_result = session_controller.run(&mut readline);
    if let Err(error) = interactive_result {
        eprintln!("Interactive session aborted with error {error}");
    }
    println!("Interactive session finished, bye.");
}
